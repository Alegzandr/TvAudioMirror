using System;
using System.Diagnostics;
using TvAudioMirror.Infrastructure.Processes;
using TvAudioMirror.Properties;

namespace TvAudioMirror.Infrastructure.Sound
{
    internal sealed class SoundSettingsLauncher
    {
        private readonly IProcessLauncher launcher;
        private readonly Action<string> log;

        public SoundSettingsLauncher(IProcessLauncher launcher, Action<string> log)
        {
            this.launcher = launcher;
            this.log = log;
        }

        public void Open()
        {
            Exception? lastError = null;
            string? lastTarget = null;
            if (TryLaunch("ms-settings:sound", ref lastError, ref lastTarget))
                return;

            if (TryLaunch("mmsys.cpl", ref lastError, ref lastTarget))
                return;

            if (lastError != null)
                log($"{Resources.Log_OpenSoundError} ({lastTarget ?? "unknown"}) {lastError.Message}");
        }

        private bool TryLaunch(string target, ref Exception? lastError, ref string? lastTarget)
        {
            try
            {
                launcher.Start(new ProcessStartInfo(target) { UseShellExecute = true });
                return true;
            }
            catch (Exception ex)
            {
                lastError = ex;
                lastTarget = target;
                return false;
            }
        }
    }
}
