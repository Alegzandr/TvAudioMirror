using System;
using System.Collections.Generic;
using System.Diagnostics;
using TvAudioMirror.Infrastructure.Processes;
using TvAudioMirror.Infrastructure.Sound;
using TvAudioMirror.Properties;
using Xunit;

namespace TvAudioMirror.Tests
{
    public class SoundSettingsLauncherTests
    {
        private sealed class FakeLauncher : IProcessLauncher
        {
            private readonly Dictionary<string, bool> behavior;

            public List<string> Attempts { get; } = new();

            public FakeLauncher(Dictionary<string, bool> behavior)
            {
                this.behavior = behavior;
            }

            public void Start(ProcessStartInfo startInfo)
            {
                var name = startInfo.FileName ?? string.Empty;
                Attempts.Add(name);
                if (!behavior.TryGetValue(name, out var succeed) || !succeed)
                    throw new InvalidOperationException(name);
            }
        }

        [Fact]
        public void UsesModernSoundSettingsWhenAvailable()
        {
            var launcher = new FakeLauncher(new Dictionary<string, bool>
            {
                { "ms-settings:sound", true }
            });
            var logs = new List<string>();
            var sut = new SoundSettingsLauncher(launcher, logs.Add);

            sut.Open();

            Assert.Equal(new[] { "ms-settings:sound" }, launcher.Attempts);
            Assert.Empty(logs);
        }

        [Fact]
        public void FallsBackToLegacyPanelWhenModernUriFails()
        {
            var launcher = new FakeLauncher(new Dictionary<string, bool>
            {
                { "ms-settings:sound", false },
                { "mmsys.cpl", true }
            });
            var logs = new List<string>();
            var sut = new SoundSettingsLauncher(launcher, logs.Add);

            sut.Open();

            Assert.Equal(new[] { "ms-settings:sound", "mmsys.cpl" }, launcher.Attempts);
            Assert.Empty(logs);
        }

        [Fact]
        public void LogsOnceWhenBothLaunchesFail()
        {
            var launcher = new FakeLauncher(new Dictionary<string, bool>());
            var logs = new List<string>();
            var sut = new SoundSettingsLauncher(launcher, logs.Add);

            sut.Open();

            Assert.Equal(new[] { "ms-settings:sound", "mmsys.cpl" }, launcher.Attempts);
            Assert.Single(logs);
        }
    }
}
