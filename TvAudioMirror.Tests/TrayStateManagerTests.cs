using TvAudioMirror.UI.Tray;
using Xunit;

namespace TvAudioMirror.Tests
{
    public class TrayStateManagerTests
    {
        [Fact]
        public void StartsHiddenWhenRequested()
        {
            var state = new TrayStateManager(startInTray: true);

            Assert.False(state.WindowVisible);
            Assert.True(state.TrayIconVisible);
        }

        [Fact]
        public void HideToTrayUpdatesVisibility()
        {
            var state = new TrayStateManager(startInTray: false);

            state.HideToTray();

            Assert.False(state.WindowVisible);
            Assert.True(state.TrayIconVisible);
        }

        [Fact]
        public void ShowFromTrayResetsIconWhenNotStartInTray()
        {
            var state = new TrayStateManager(startInTray: false);
            state.HideToTray();

            state.ShowFromTray();

            Assert.True(state.WindowVisible);
            Assert.False(state.TrayIconVisible);
        }
    }
}
