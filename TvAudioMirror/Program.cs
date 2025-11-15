using System;
using System.Globalization;
using System.Threading;
using System.Windows.Forms;
using TvAudioMirror.Core.Startup;

namespace TvAudioMirror
{
    internal static class Program
    {
        [STAThread]
        static void Main(string[] args)
        {
            var ui = CultureInfo.CurrentUICulture;
            CultureInfo.DefaultThreadCurrentCulture = ui;
            CultureInfo.DefaultThreadCurrentUICulture = ui;
            Thread.CurrentThread.CurrentCulture = ui;
            Thread.CurrentThread.CurrentUICulture = ui;

            ApplicationConfiguration.Initialize();

            bool startInTray = StartupOptions.ShouldStartInTray(args);

            Application.Run(new MainForm(startInTray));
        }
    }
}
