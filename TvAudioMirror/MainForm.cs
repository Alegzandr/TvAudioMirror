using System;
using System.Drawing;
using System.Windows.Forms;
using TvAudioMirror.Core.Audio;
using TvAudioMirror.Core.Devices;
using TvAudioMirror.Core.Mirroring;
using TvAudioMirror.Infrastructure.Logging;
using TvAudioMirror.Infrastructure.Processes;
using TvAudioMirror.Infrastructure.Sound;
using TvAudioMirror.Properties;
using TvAudioMirror.UI.Tray;

namespace TvAudioMirror
{
    public sealed class MainForm : Form
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
        private readonly TrayStateManager trayState;
        private readonly IProcessLauncher processLauncher;
        private readonly SoundSettingsLauncher soundSettingsLauncher;
        private readonly AudioMirrorCoordinator mirrorCoordinator;
        private readonly ILogSink logSink;

        private bool isClosing;

        public MainForm(bool startInTray = false)
            : this(startInTray, null, null, null)
        {
        }

        internal MainForm(
            bool startInTray,
            IProcessLauncher? processLauncher,
            AudioMirrorCoordinator? coordinator,
            ILogSink? logSink)
        {
            this.startInTray = startInTray;
            this.processLauncher = processLauncher ?? new ProcessLauncher();
            trayState = new TrayStateManager(startInTray);
            this.logSink = logSink ?? new DelegateLogSink(AppendLog);

            if (coordinator == null)
            {
                var pipelineLog = new Action<string>(message =>
                    this.logSink.Publish(LogEvent.Create(LogLevel.Debug, message)));
                var pipeline = new AudioPipeline(pipelineLog);
                var catalog = new WasapiDeviceCatalog();
                mirrorCoordinator = new AudioMirrorCoordinator(catalog, pipeline, this.logSink);
            }
            else
            {
                mirrorCoordinator = coordinator;
            }

            mirrorCoordinator.StateChanged += MirrorCoordinator_StateChanged;
            mirrorCoordinator.TvVolumeChanged += MirrorCoordinator_TvVolumeChanged;
            mirrorCoordinator.TvMuteChanged += MirrorCoordinator_TvMuteChanged;

            soundSettingsLauncher = new SoundSettingsLauncher(
                this.processLauncher,
                message => Log(LogLevel.Error, message));

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
            tbVolume.Scroll += (_, _) => mirrorCoordinator.SetTvVolume(tbVolume!.Value / 100f);

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
            trayMenu.Items.Add(Resources.Tray_Reload, null, (_, _) => ReloadApplication());
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

            cbAuto.CheckedChanged += (_, _) =>
            {
                if (cbAuto == null) return;
                mirrorCoordinator.SetAutoRefresh(cbAuto.Checked);
                Log(LogLevel.Info, cbAuto.Checked ? "Auto-refresh enabled." : "Auto-refresh paused.");
            };

            mirrorCoordinator.Initialize();
            mirrorCoordinator.SetAutoRefresh(cbAuto.Checked);

            FormClosing += MainForm_FormClosing;
            Resize += MainForm_Resize;
        }

        private void MirrorCoordinator_StateChanged(object? sender, AudioMirrorState state)
        {
            if (state.DefaultDevice.HasValue)
            {
                SetLabelText(lblSource, string.Format(Resources.Label_Source_Value, state.DefaultDevice.Value.FriendlyName));
            }
            else
            {
                SetLabelText(lblSource, Resources.Label_Source_Unknown);
            }

            string tvText = state.Status switch
            {
                AudioMirrorStatus.Mirroring when state.TvDevice.HasValue =>
                    string.Format(Resources.Label_Tv_Value, state.TvDevice.Value.FriendlyName),
                AudioMirrorStatus.DefaultIsTv => Resources.Label_Tv_DefaultIsTV,
                AudioMirrorStatus.TvNotFound => Resources.Label_Tv_NotFound,
                AudioMirrorStatus.Error when !string.IsNullOrWhiteSpace(state.ErrorMessage) => state.ErrorMessage!,
                _ => Resources.Label_Tv_NotFound
            };

            SetLabelText(lblTv, tvText);
        }

        private void MirrorCoordinator_TvVolumeChanged(object? sender, float scalar)
        {
            if (tbVolume == null) return;
            var percent = (int)Math.Round(Math.Clamp(scalar, 0f, 1f) * 100, MidpointRounding.AwayFromZero);
            SetTrackBarValue(tbVolume, percent);
        }

        private void MirrorCoordinator_TvMuteChanged(object? sender, bool isMuted)
        {
            if (btnMute == null) return;
            var text = isMuted ? Resources.Button_Unmute : Resources.Button_Mute;

            if (btnMute.InvokeRequired)
            {
                try
                {
                    btnMute.BeginInvoke(new Action(() => btnMute.Text = text));
                }
                catch (ObjectDisposedException) { }
                catch (InvalidOperationException) { }
            }
            else
            {
                btnMute.Text = text;
            }
        }

        private void HideToTray()
        {
            trayState.HideToTray();
            Hide();
            ShowInTaskbar = false;
            if (trayIcon != null) trayIcon.Visible = true;
        }

        private void ShowFromTray()
        {
            trayState.ShowFromTray();
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

        private void ReloadApplication()
        {
            try
            {
                Log(LogLevel.Info, "Reloading application...");
                isClosing = true;
                Application.Restart();
                Environment.Exit(0);
            }
            catch (Exception ex)
            {
                isClosing = false;
                Log(LogLevel.Error, "Reload failed: " + ex.Message);
            }
        }

        private void MainForm_Resize(object? sender, EventArgs e)
        {
            if (WindowState == FormWindowState.Minimized && trayState.MinimizeToTray)
                HideToTray();
        }

        private void ToggleMute()
        {
            mirrorCoordinator.ToggleMute();
        }

        private void SetLabelText(Label? label, string text)
        {
            if (label == null) return;
            if (label.InvokeRequired)
            {
                try
                {
                    label.BeginInvoke(new Action(() => label.Text = text));
                }
                catch (ObjectDisposedException) { }
                catch (InvalidOperationException) { }
            }
            else
            {
                label.Text = text;
            }
        }

        private void SetTrackBarValue(TrackBar? trackBar, int value)
        {
            if (trackBar == null) return;
            void Apply()
            {
                var clamped = Math.Clamp(value, trackBar.Minimum, trackBar.Maximum);
                trackBar.Value = clamped;
            }

            if (trackBar.InvokeRequired)
            {
                try
                {
                    trackBar.BeginInvoke(new Action(Apply));
                }
                catch (ObjectDisposedException) { }
                catch (InvalidOperationException) { }
            }
            else
            {
                Apply();
            }
        }

        private void OpenSoundSettings()
        {
            soundSettingsLauncher.Open();
        }

        private void Log(LogLevel level, string msg)
        {
            logSink.Publish(LogEvent.Create(level, msg));
        }

        private void AppendLog(LogEvent logEvent)
        {
            if (isClosing) return;
            if (txtLog == null) return;

            var prefix = logEvent.Level switch
            {
                LogLevel.Error => "[ERR] ",
                LogLevel.Warning => "[WRN] ",
                LogLevel.Debug => "[DBG] ",
                _ => "[INF] "
            };

            var line = $"[{logEvent.Timestamp:HH:mm:ss}] {prefix}{logEvent.Message}{Environment.NewLine}";

            if (txtLog.InvokeRequired)
            {
                try
                {
                    txtLog.BeginInvoke(new Action(() => txtLog.AppendText(line)));
                }
                catch (ObjectDisposedException) { }
                catch (InvalidOperationException) { }
            }
            else
            {
                txtLog.AppendText(line);
            }
        }

        private void MainForm_FormClosing(object? sender, FormClosingEventArgs e)
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
            mirrorCoordinator.Dispose();

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
}
