namespace TvAudioMirror.UI.Tray
{
    internal sealed class TrayStateManager
    {
        public bool StartInTray { get; }
        public bool MinimizeToTray { get; private set; } = true;
        public bool WindowVisible { get; private set; }
        public bool TrayIconVisible { get; private set; }

        public TrayStateManager(bool startInTray)
        {
            StartInTray = startInTray;
            WindowVisible = !startInTray;
            TrayIconVisible = startInTray;
        }

        public void HideToTray()
        {
            MinimizeToTray = true;
            WindowVisible = false;
            TrayIconVisible = true;
        }

        public void ShowFromTray()
        {
            WindowVisible = true;
            if (!StartInTray)
                TrayIconVisible = false;
        }
    }
}
