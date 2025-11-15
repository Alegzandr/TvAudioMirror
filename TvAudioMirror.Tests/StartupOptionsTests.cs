using TvAudioMirror.Core.Startup;
using Xunit;

namespace TvAudioMirror.Tests
{
    public class StartupOptionsTests
    {
        [Theory]
        [InlineData("--tray")]
        [InlineData("/tray")]
        [InlineData("/TRAY")]
        [InlineData("-x", "/tray")]
        public void DetectsTrayFlag(params string[] args)
        {
            Assert.True(StartupOptions.ShouldStartInTray(args));
        }

        [Fact]
        public void ReturnsFalseWhenFlagMissing()
        {
            var args = new[] { "--other", "/noop" };

            Assert.False(StartupOptions.ShouldStartInTray(args));
        }

        [Fact]
        public void ThrowsWhenArgsNull()
        {
            Assert.Throws<System.ArgumentNullException>(() => StartupOptions.ShouldStartInTray(null!));
        }
    }
}
