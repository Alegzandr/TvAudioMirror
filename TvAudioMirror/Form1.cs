using System;
using System.Diagnostics;
using System.Drawing;
using System.Windows.Forms;
using NAudio.CoreAudioApi;
using NAudio.CoreAudioApi.Interfaces;
using NAudio.Wave;
using TvAudioMirror.Properties;

namespace TvAudioMirror
{
    public partial class Form1 : Form
    {
        private Label? lblInfo;
        private Label? lblSource;
        private Label? lblTv;
        private Button? btnMute;
        private TrackBar? tbVolume;
        private CheckBox? cbAuto;
        private Button? btnSoundSettings;
        private TextBox? txtLog;

        private NotifyIcon? trayIcon;
        private ContextMenuStrip? trayMenu;
        private readonly bool startInTray;
        private bool minimizeToTray = true;

        private readonly MMDeviceEnumerator enumerator = new();
        private MMDevice? currentDefault;
        private MMDevice? currentTv;
        private WasapiLoopbackCapture? capture;
        private WasapiOut? tvOut;
        private BufferedWaveProvider? buffer;
        private readonly object sync = new();
        private bool isClosing;

        private AudioDeviceNotification? notificationClient;

        public Form1(bool startInTray = false)
        {
            this.startInTray = startInTray;
            InitializeComponent();
            InitializeCustomUi();
        }

        private void InitializeCustomUi()
        {
            Text = Resources.App_Title;
            Width = 560;
            Height = 420;
            StartPosition = FormStartPosition.CenterScreen;
            FormBorderStyle = FormBorderStyle.FixedSingle;
            MaximizeBox = false;
            MinimizeBox = true;
            SizeGripStyle = SizeGripStyle.Hide;

            try { Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); }
            catch { Icon = SystemIcons.Application; }

            lblInfo = new Label
            {
                Left = 15,
                Top = 10,
                Width = 520,
                Height = 50,
                Text = Resources.Info_Text
            };

            lblSource = new Label
            {
                Left = 15,
                Top = 65,
                Width = 520,
                Text = Resources.Label_Source_Unknown
            };

            lblTv = new Label
            {
                Left = 15,
                Top = 90,
                Width = 520,
                Text = Resources.Label_Tv_NotFound
            };

            btnMute = new Button
            {
                Left = 15,
                Top = 120,
                Width = 100,
                Text = Resources.Button_Mute
            };
            btnMute.Click += (_, _) => ToggleMute();

            tbVolume = new TrackBar
            {
                Left = 130,
                Top = 118,
                Width = 250,
                Minimum = 0,
                Maximum = 100,
                TickFrequency = 10,
                Value = 80
            };
            tbVolume.Scroll += (_, _) => SetTvVolume(tbVolume!.Value / 100f);

            cbAuto = new CheckBox
            {
                Left = 15,
                Top = 160,
                Width = 350,
                Text = Resources.Check_Auto,
                Checked = true
            };

            btnSoundSettings = new Button
            {
                Left = 380,
                Top = 155,
                Width = 150,
                Height = 28,
                Text = Resources.Button_SoundSettings
            };
            btnSoundSettings.Click += (_, _) => OpenSoundSettings();

            txtLog = new TextBox
            {
                Left = 15,
                Top = 185,
                Width = 520,
                Height = 180,
                Multiline = true,
                ScrollBars = ScrollBars.Vertical,
                ReadOnly = true
            };

            Controls.Add(lblInfo);
            Controls.Add(lblSource);
            Controls.Add(lblTv);
            Controls.Add(btnMute);
            Controls.Add(tbVolume);
            Controls.Add(cbAuto);
            Controls.Add(btnSoundSettings);
            Controls.Add(txtLog);

            trayMenu = new ContextMenuStrip();
            trayMenu.Items.Add(Resources.Tray_Open, null, (_, _) => ShowFromTray());
            trayMenu.Items.Add(Resources.Tray_Exit, null, (_, _) => CloseFromTray());

            trayIcon = new NotifyIcon
            {
                Text = Resources.App_Title,
                Visible = startInTray,
                ContextMenuStrip = trayMenu
            };
            try { trayIcon.Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); }
            catch { trayIcon.Icon = SystemIcons.Application; }
            trayIcon.DoubleClick += (_, _) => ShowFromTray();

            notificationClient = new AudioDeviceNotification(() =>
            {
                if (cbAuto != null && cbAuto.Checked)
                {
                    Log("[info] Default device changed → refreshing...");
                    RefreshDevices();
                }
            });
            enumerator.RegisterEndpointNotificationCallback(notificationClient);

            RefreshDevices();

            FormClosing += Form1_FormClosing;
            Resize += Form1_Resize;
        }

        private void HideToTray()
        {
            minimizeToTray = true;
            Hide();
            ShowInTaskbar = false;
            if (trayIcon != null) trayIcon.Visible = true;
        }

        private void ShowFromTray()
        {
            Show();
            WindowState = FormWindowState.Normal;
            ShowInTaskbar = true;
            Activate();
            if (trayIcon != null && !startInTray)
                trayIcon.Visible = false;
        }

        private void CloseFromTray()
        {
            isClosing = true;
            Close();
        }

        private void Form1_Resize(object? sender, EventArgs e)
        {
            if (WindowState == FormWindowState.Minimized && minimizeToTray)
                HideToTray();
        }

        private MMDevice GetDefaultRender() =>
            enumerator.GetDefaultAudioEndpoint(DataFlow.Render, Role.Multimedia);

        private MMDevice? FindTv()
        {
            var devs = enumerator.EnumerateAudioEndPoints(DataFlow.Render, DeviceState.Active);
            foreach (var d in devs)
                if (d.FriendlyName.IndexOf("tv", StringComparison.OrdinalIgnoreCase) >= 0)
                    return d;
            return null;
        }

        private static bool SameDevice(MMDevice? a, MMDevice? b)
        {
            if (a == null && b == null) return true;
            if (a == null || b == null) return false;
            return a.ID == b.ID;
        }

        private void RefreshDevices()
        {
            lock (sync)
            {
                StopPipeline();

                MMDevice def;
                try
                {
                    def = GetDefaultRender();
                }
                catch (Exception ex)
                {
                    Log(Resources.Log_DefaultReadError + " " + ex.Message);
                    if (lblSource != null) lblSource.Text = Resources.Label_Source_Error;
                    return;
                }

                currentDefault = def;
                if (lblSource != null)
                    lblSource.Text = string.Format(Resources.Label_Source_Value, def.FriendlyName);

                if (def.FriendlyName.IndexOf("tv", StringComparison.OrdinalIgnoreCase) >= 0)
                {
                    Log(Resources.Log_DefaultIsTv);
                    if (lblTv != null) lblTv.Text = Resources.Label_Tv_DefaultIsTV;
                    return;
                }

                var tv = FindTv();
                if (tv == null)
                {
                    Log(Resources.Log_NoTvFound);
                    if (lblTv != null) lblTv.Text = Resources.Label_Tv_NotFound;
                    return;
                }

                currentTv = tv;
                if (lblTv != null) lblTv.Text = string.Format(Resources.Label_Tv_Value, tv.FriendlyName);

                try
                {
                    StartPipeline(def, tv);
                    Log(string.Format(Resources.Log_CaptureStarted, def.FriendlyName, tv.FriendlyName));
                }
                catch (Exception ex)
                {
                    Log(Resources.Log_CaptureError + " " + ex.Message);
                }
            }
        }

        private void StartPipeline(MMDevice source, MMDevice tv)
        {
            capture = new WasapiLoopbackCapture(source);
            buffer = new BufferedWaveProvider(capture.WaveFormat)
            {
                DiscardOnBufferOverflow = true,
                BufferDuration = TimeSpan.FromMilliseconds(100)
            };
            var minBufferBytes = capture.WaveFormat.AverageBytesPerSecond / 15;
            buffer.BufferLength = Math.Max(minBufferBytes, capture.WaveFormat.BlockAlign * 16);

            tvOut = CreateTvOutput(tv);
            tvOut.Init(buffer);
            tvOut.Play();

            capture.DataAvailable += (_, a) =>
            {
                buffer!.AddSamples(a.Buffer, 0, a.BytesRecorded);
            };

            capture.RecordingStopped += (_, _) =>
            {
                try { tvOut?.Stop(); } catch { }
            };

            capture.StartRecording();

            if (tbVolume != null)
            {
                try
                {
                    var vol = tv.AudioEndpointVolume.MasterVolumeLevelScalar;
                    tbVolume.Value = (int)(vol * 100);
                }
                catch { }
            }
        }

        private WasapiOut CreateTvOutput(MMDevice tv)
        {
            try
            {
                var exclusive = new WasapiOut(tv, AudioClientShareMode.Exclusive, true, 10);
                Log("Using exclusive audio mode for TV output.");
                return exclusive;
            }
            catch (Exception ex)
            {
                Log("Exclusive audio mode unavailable, falling back to shared mode. " + ex.Message);
            }

            var shared = new WasapiOut(tv, AudioClientShareMode.Shared, true, 35);
            Log("Using shared audio mode for TV output.");
            return shared;
        }

        private void StopPipeline()
        {
            try { capture?.StopRecording(); } catch { }
            try { capture?.Dispose(); } catch { }
            capture = null;

            try { tvOut?.Stop(); } catch { }
            try { tvOut?.Dispose(); } catch { }
            tvOut = null;

            buffer = null;
        }

        private void ToggleMute()
        {
            lock (sync)
            {
                if (currentTv == null) return;
                try
                {
                    var vol = currentTv.AudioEndpointVolume;
                    vol.Mute = !vol.Mute;
                    Log(string.Format(Resources.Log_Mute, vol.Mute));
                }
                catch (Exception ex)
                {
                    Log(Resources.Log_MuteError + " " + ex.Message);
                }
            }
        }

        private void SetTvVolume(float v)
        {
            lock (sync)
            {
                if (currentTv == null) return;
                v = Math.Clamp(v, 0f, 1f);
                try
                {
                    currentTv.AudioEndpointVolume.MasterVolumeLevelScalar = v;
                    Log(string.Format(Resources.Log_Volume, (int)(v * 100)));
                }
                catch (Exception ex)
                {
                    Log(Resources.Log_VolumeError + " " + ex.Message);
                }
            }
        }

        private void OpenSoundSettings()
        {
            try
            {
                Process.Start(new ProcessStartInfo("ms-settings:sound") { UseShellExecute = true });
            }
            catch
            {
                try
                {
                    Process.Start(new ProcessStartInfo("mmsys.cpl") { UseShellExecute = true });
                }
                catch (Exception ex)
                {
                    Log(Resources.Log_OpenSoundError + " " + ex.Message);
                }
            }
        }

        private void Log(string msg)
        {
            if (isClosing) return;
            if (txtLog == null) return;

            if (txtLog.InvokeRequired)
            {
                txtLog.Invoke(new Action(() => Log(msg)));
                return;
            }

            txtLog.AppendText($"[{DateTime.Now:HH:mm:ss}] {msg}{Environment.NewLine}");
        }

        private void Form1_FormClosing(object? sender, FormClosingEventArgs e)
        {
            if (e.CloseReason == CloseReason.UserClosing && !isClosing)
            {
                var res = MessageBox.Show(
                    Resources.Confirm_Exit_Text,
                    Resources.Confirm_Exit_Title,
                    MessageBoxButtons.YesNo,
                    MessageBoxIcon.Question);

                if (res == DialogResult.No)
                {
                    e.Cancel = true;
                    return;
                }
            }

            isClosing = true;
            StopPipeline();

            if (notificationClient != null)
            {
                try { enumerator.UnregisterEndpointNotificationCallback(notificationClient); } catch { }
            }

            if (trayIcon != null)
            {
                trayIcon.Visible = false;
                trayIcon.Dispose();
            }
        }

        protected override void OnShown(EventArgs e)
        {
            base.OnShown(e);
            if (startInTray)
                HideToTray();
        }
    }

    public class AudioDeviceNotification : IMMNotificationClient
    {
        private readonly Action onDefaultChanged;

        public AudioDeviceNotification(Action onDefaultChanged)
        {
            this.onDefaultChanged = onDefaultChanged;
        }

        public void OnDefaultDeviceChanged(DataFlow flow, Role role, string defaultDeviceId)
        {
            if (flow == DataFlow.Render && role == Role.Multimedia)
                onDefaultChanged();
        }

        public void OnDeviceAdded(string pwstrDeviceId) { }
        public void OnDeviceRemoved(string pwstrDeviceId) { }
        public void OnDeviceStateChanged(string pwstrDeviceId, DeviceState newState) { }
        public void OnPropertyValueChanged(string pwstrDeviceId, PropertyKey key) { }
    }
}
