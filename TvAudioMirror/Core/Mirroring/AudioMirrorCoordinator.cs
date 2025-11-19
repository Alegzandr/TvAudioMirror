using System;
using NAudio.CoreAudioApi;
using TvAudioMirror.Core.Audio;
using TvAudioMirror.Core.Devices;
using TvAudioMirror.Infrastructure.Logging;
using TvAudioMirror.Infrastructure.Sound;
using TvAudioMirror.Properties;

namespace TvAudioMirror.Core.Mirroring
{
    internal sealed class AudioMirrorCoordinator : IDisposable
    {
        private readonly IAudioDeviceCatalog devices;
        private readonly IAudioPipeline pipeline;
        private readonly ILogSink logSink;
        private readonly object gate = new(); // serializes pipeline/device mutations
        private readonly Action<LogLevel, string> log;

        private AudioDeviceNotification? notificationClient;
        private bool disposed;
        private bool initialized;
        private bool autoRefreshEnabled = true;
        private AudioMirrorState currentState = AudioMirrorState.Idle();
        private MMDevice? currentTv;
        private bool? currentMute;
        private DateTime lastRefreshTime = DateTime.MinValue;

        public AudioMirrorCoordinator(
            IAudioDeviceCatalog devices,
            IAudioPipeline pipeline,
            ILogSink logSink)
        {
            this.devices = devices ?? throw new ArgumentNullException(nameof(devices));
            this.pipeline = pipeline ?? throw new ArgumentNullException(nameof(pipeline));
            this.logSink = logSink ?? throw new ArgumentNullException(nameof(logSink));
            log = (level, message) => logSink.Publish(LogEvent.Create(level, message));
        }

        public event EventHandler<AudioMirrorState>? StateChanged;
        public event EventHandler<float>? TvVolumeChanged;
        public event EventHandler<bool>? TvMuteChanged;

        public bool AutoRefreshEnabled
        {
            get
            {
                lock (gate) return autoRefreshEnabled;
            }
        }

        public AudioMirrorState CurrentState
        {
            get
            {
                lock (gate) return currentState;
            }
        }

        public void Initialize()
        {
            lock (gate)
            {
                if (disposed || initialized)
                    return;

                notificationClient = new AudioDeviceNotification(OnDefaultDeviceChanged);
                devices.RegisterNotification(notificationClient);
                initialized = true;
            }

            RefreshDevices();
        }

        public void SetAutoRefresh(bool enabled)
        {
            lock (gate)
            {
                autoRefreshEnabled = enabled;
            }

            if (enabled)
                RefreshDevices();
        }

        private void OnDefaultDeviceChanged()
        {
            if (!AutoRefreshEnabled)
            {
                log(LogLevel.Debug, "Default render device changed but auto-refresh is disabled.");
                return;
            }

            log(LogLevel.Debug, "Default render device changed; rebuilding pipeline.");
            RefreshDevices();
        }

        public void RefreshDevices()
        {
            AudioMirrorState? newState = null;
            float? reportedVolume = null;
            bool? muteState = null;

            lock (gate)
            {
                if (disposed) return;

                // Debounce: ignore refreshes within 1 second of last refresh
                var timeSinceLastRefresh = DateTime.UtcNow - lastRefreshTime;
                if (timeSinceLastRefresh.TotalMilliseconds < 1000)
                {
                    log(LogLevel.Debug, "Refresh skipped (debounce).");
                    return;
                }
                lastRefreshTime = DateTime.UtcNow;

                try
                {
                    var defaultDevice = devices.GetDefaultRender();
                    var defaultInfo = DeviceInfo.FromMmDevice(defaultDevice);
                    var tv = devices.FindTv();
                    var tvInfo = tv != null ? DeviceInfo.FromMmDevice(tv) : (DeviceInfo?)null;
                    var decision = DeviceRefreshDecider.Decide(defaultInfo, tvInfo);

                    pipeline.Stop();
                    currentTv = null;
                    currentMute = null;

                    switch (decision.Outcome)
                    {
                        case RefreshOutcome.DefaultIsTv:
                            log(LogLevel.Info, Resources.Log_DefaultIsTv);
                            newState = AudioMirrorState.DefaultIsTv(defaultInfo);
                            muteState = false;
                            break;
                        case RefreshOutcome.TvNotFound:
                            log(LogLevel.Warning, Resources.Log_NoTvFound);
                            newState = AudioMirrorState.TvNotFound(defaultInfo);
                            muteState = false;
                            break;
                        case RefreshOutcome.StartPipeline:
                            if (tv == null)
                            {
                                log(LogLevel.Warning, Resources.Log_NoTvFound);
                                newState = AudioMirrorState.TvNotFound(defaultInfo);
                                muteState = false;
                                break;
                            }

                            try
                            {
                                pipeline.Start(defaultDevice, tv);
                                currentTv = tv;
                                log(LogLevel.Info, string.Format(Resources.Log_CaptureStarted, defaultDevice.FriendlyName, tv.FriendlyName));
                                newState = AudioMirrorState.Mirroring(defaultInfo, DeviceInfo.FromMmDevice(tv));

                                try
                                {
                                    reportedVolume = tv.AudioEndpointVolume.MasterVolumeLevelScalar;
                                }
                                catch
                                {
                                    // ignore volume read errors
                                }

                                try
                                {
                                    muteState = tv.AudioEndpointVolume.Mute;
                                }
                                catch
                                {
                                    muteState = false;
                                }
                            }
                            catch (Exception ex)
                            {
                                log(LogLevel.Error, Resources.Log_CaptureError + " " + ex.Message);
                                newState = AudioMirrorState.Error(defaultInfo, ex.Message);
                                muteState = false;
                            }

                            break;
                    }
                }
                catch (Exception ex)
                {
                    log(LogLevel.Error, Resources.Log_DefaultReadError + " " + ex.Message);
                    newState = AudioMirrorState.Error(null, ex.Message);
                    muteState = false;
                }

                if (newState.HasValue)
                    currentState = newState.Value;

                if (muteState.HasValue)
                    currentMute = muteState.Value;
            }

            if (newState.HasValue)
                StateChanged?.Invoke(this, newState.Value);

            if (reportedVolume.HasValue)
                TvVolumeChanged?.Invoke(this, reportedVolume.Value);

            if (muteState.HasValue)
                TvMuteChanged?.Invoke(this, muteState.Value);
        }

        public void ToggleMute()
        {
            bool? muteState = null;

            lock (gate)
            {
                if (currentTv == null) return;

                try
                {
                    var volume = new AudioEndpointVolumeAdapter(currentTv.AudioEndpointVolume);
                    var muted = EndpointVolumeController.ToggleMute(volume);
                    currentMute = muted;
                    muteState = muted;
                    log(LogLevel.Info, string.Format(Resources.Log_Mute, muted));
                }
                catch (Exception ex)
                {
                    log(LogLevel.Error, Resources.Log_MuteError + " " + ex.Message);
                }
            }

            if (muteState.HasValue)
                TvMuteChanged?.Invoke(this, muteState.Value);
        }

        public void SetTvVolume(float scalar)
        {
            int? percent = null;
            float? appliedScalar = null;

            lock (gate)
            {
                if (currentTv == null) return;

                try
                {
                    var volume = new AudioEndpointVolumeAdapter(currentTv.AudioEndpointVolume);
                    percent = EndpointVolumeController.SetVolume(volume, scalar);
                    appliedScalar = Math.Clamp(scalar, 0f, 1f);
                    log(LogLevel.Info, string.Format(Resources.Log_Volume, percent));
                }
                catch (Exception ex)
                {
                    percent = null;
                    appliedScalar = null;
                    log(LogLevel.Error, Resources.Log_VolumeError + " " + ex.Message);
                }
            }

            if (percent.HasValue && appliedScalar.HasValue)
                TvVolumeChanged?.Invoke(this, appliedScalar.Value);
        }

        public void Dispose()
        {
            lock (gate)
            {
                if (disposed) return;
                disposed = true;
                pipeline.Stop();
                pipeline.Dispose();

                if (notificationClient != null)
                {
                    devices.UnregisterNotification(notificationClient);
                }
            }
        }
    }
}
