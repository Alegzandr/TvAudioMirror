using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using NAudio.CoreAudioApi;
using TvAudioMirror.Core.Audio;
using TvAudioMirror.Core.Devices;
using TvAudioMirror.Core.Startup;
using TvAudioMirror.Infrastructure.Processes;
using TvAudioMirror.Infrastructure.Sound;
using TvAudioMirror.Properties;
using TvAudioMirror.UI.Tray;
using Xunit;

namespace TvAudioMirror.Tests
{
    public class AppFeatureTestPlan
    {
        [Fact]
        public void MirrorsDefaultRenderDeviceToFirstTvDevice()
        {
            var decision = DeviceRefreshDecider.Decide(
                new DeviceInfo("default", "Desktop Speakers"),
                new DeviceInfo("tv-1", "Living Room TV"));

            Assert.Equal(RefreshOutcome.StartPipeline, decision.Outcome);
            Assert.Equal("tv-1", decision.TvDevice?.Id);
        }

        [Fact]
        public void SkipsMirrorWhenDefaultDeviceIsAlreadyTv()
        {
            var decision = DeviceRefreshDecider.Decide(
                new DeviceInfo("default", "Samsung TV"),
                null);

            Assert.Equal(RefreshOutcome.DefaultIsTv, decision.Outcome);
        }

        [Fact]
        public void RebuildsPipelineWhenDefaultDeviceChanges()
        {
            var refreshes = 0;
            var notification = new AudioDeviceNotification(() => refreshes++);

            notification.OnDefaultDeviceChanged(DataFlow.Render, Role.Multimedia, "default");
            notification.OnDefaultDeviceChanged(DataFlow.Capture, Role.Multimedia, "default");

            Assert.Equal(1, refreshes);
        }

        [Fact]
        public void ShowsTvNotFoundWhenNoMatchingDeviceExists()
        {
            var decision = DeviceRefreshDecider.Decide(
                new DeviceInfo("default", "USB Headset"),
                null);

            Assert.Equal(RefreshOutcome.TvNotFound, decision.Outcome);
        }

        [Fact]
        public void MuteButtonTogglesTvEndpointMuteState()
        {
            var volume = new FakeVolume { Mute = false };

            var muted = EndpointVolumeController.ToggleMute(volume);

            Assert.True(muted);
            Assert.True(volume.Mute);
        }

        [Fact]
        public void VolumeSliderAdjustsTvMasterVolume()
        {
            var volume = new FakeVolume { MasterVolumeLevelScalar = 0.1f };

            var percent = EndpointVolumeController.SetVolume(volume, 0.65f);

            Assert.Equal(65, percent);
            Assert.Equal(0.65f, volume.MasterVolumeLevelScalar, 3);
        }

        [Fact]
        public void OpenSoundSettingsFallsBackToLegacyPanelWhenModernUriFails()
        {
            var launcher = new SequencedProcessLauncher(new[]
            {
                (Target: "ms-settings:sound", Success: false),
                (Target: "mmsys.cpl", Success: true)
            });
            var logs = new List<string>();
            var sut = new SoundSettingsLauncher(launcher, logs.Add);

            sut.Open();

            Assert.Equal(new[] { "ms-settings:sound", "mmsys.cpl" }, launcher.Attempts);
            Assert.Empty(logs);
        }

        [Fact]
        public void TrayHideShowFlow()
        {
            var tray = new TrayStateManager(startInTray: false);

            tray.HideToTray();
            Assert.False(tray.WindowVisible);
            Assert.True(tray.TrayIconVisible);

            tray.ShowFromTray();
            Assert.True(tray.WindowVisible);
            Assert.False(tray.TrayIconVisible);
        }

        [Fact]
        public void StartsInTrayWhenRequested()
        {
            var args = new[] { "--tray" };
            Assert.True(StartupOptions.ShouldStartInTray(args));

            var tray = new TrayStateManager(startInTray: true);
            Assert.False(tray.WindowVisible);
            Assert.True(tray.TrayIconVisible);
        }

        [Fact]
        public void LocalizationSwitchesStringsPerCulture()
        {
            var targetCulture = CultureInfo.GetCultureInfo("fr");
            var original = Resources.Culture;
            try
            {
                Resources.Culture = targetCulture;
                Assert.False(string.IsNullOrWhiteSpace(Resources.App_Title));
                Resources.Culture = CultureInfo.GetCultureInfo("es");
                Assert.False(string.IsNullOrWhiteSpace(Resources.App_Title));
            }
            finally
            {
                Resources.Culture = original;
            }
        }

        [Theory]
        [InlineData("")]
        [InlineData("fr")]
        [InlineData("es")]
        [InlineData("de")]
        [InlineData("it")]
        [InlineData("pt-BR")]
        [InlineData("zh-Hans")]
        [InlineData("ru")]
        [InlineData("ja")]
        [InlineData("ko")]
        public void LocalizedResourcesContainAppTitle(string cultureName)
        {
            var culture = cultureName == string.Empty ? CultureInfo.InvariantCulture : CultureInfo.GetCultureInfo(cultureName);
            var original = Resources.Culture;
            try
            {
                Resources.Culture = cultureName == string.Empty ? null : culture;
                var title = Resources.App_Title;
                Assert.False(string.IsNullOrWhiteSpace(title), $"Missing App_Title for culture '{cultureName}'.");
            }
            finally
            {
                Resources.Culture = original;
            }
        }

        private sealed class FakeVolume : IEndpointVolume
        {
            public bool Mute { get; set; }
            public float MasterVolumeLevelScalar { get; set; }
        }

        private sealed class SequencedProcessLauncher : IProcessLauncher
        {
            private readonly Queue<(string Target, bool Success)> sequence;

            public List<string> Attempts { get; } = new();

            public SequencedProcessLauncher(IEnumerable<(string Target, bool Success)> sequence)
            {
                this.sequence = new Queue<(string Target, bool Success)>(sequence);
            }

            public void Start(ProcessStartInfo startInfo)
            {
                var fileName = startInfo.FileName ?? string.Empty;
                Attempts.Add(fileName);
                if (sequence.Count == 0)
                    throw new InvalidOperationException("No behavior configured.");

                var behavior = sequence.Dequeue();
                if (behavior.Target != fileName)
                    throw new InvalidOperationException("Unexpected target requested.");

                if (!behavior.Success)
                    throw new InvalidOperationException(fileName);
            }
        }
    }
}
