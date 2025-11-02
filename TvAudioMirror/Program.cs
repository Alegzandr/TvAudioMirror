using System;
using System.Globalization;
using System.Linq;
using System.Threading;
using System.Windows.Forms;

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

            bool startInTray = args.Any(a =>
                a.Equals("--tray", StringComparison.OrdinalIgnoreCase) ||
                a.Equals("/tray", StringComparison.OrdinalIgnoreCase));

            Application.Run(new Form1(startInTray));
        }
    }
}
