using System;
using System.Collections.Generic;

namespace TvAudioMirror.Core.Startup
{
    internal static class StartupOptions
    {
        public static bool ShouldStartInTray(IEnumerable<string> args)
        {
            if (args == null) throw new ArgumentNullException(nameof(args));

            foreach (var arg in args)
            {
                if (arg.Equals("--tray", StringComparison.OrdinalIgnoreCase) ||
                    arg.Equals("/tray", StringComparison.OrdinalIgnoreCase))
                {
                    return true;
                }
            }

            return false;
        }
    }
}
