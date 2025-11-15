using System.Diagnostics;

namespace TvAudioMirror.Infrastructure.Processes
{
    internal interface IProcessLauncher
    {
        void Start(ProcessStartInfo startInfo);
    }

    internal sealed class ProcessLauncher : IProcessLauncher
    {
        public void Start(ProcessStartInfo startInfo)
        {
            Process.Start(startInfo);
        }
    }
}
