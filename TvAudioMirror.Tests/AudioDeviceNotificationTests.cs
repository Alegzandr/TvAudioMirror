using NAudio.CoreAudioApi;
using TvAudioMirror.Infrastructure.Sound;
using Xunit;

namespace TvAudioMirror.Tests
{
    public class AudioDeviceNotificationTests
    {
        [Fact]
        public void InvokesCallbackForRenderMultimedia()
        {
            bool invoked = false;
            var notification = new AudioDeviceNotification(() => invoked = true);

            notification.OnDefaultDeviceChanged(DataFlow.Render, Role.Multimedia, "id");

            Assert.True(invoked);
        }

        [Theory]
        [InlineData(DataFlow.Capture, Role.Multimedia)]
        [InlineData(DataFlow.Render, Role.Console)]
        [InlineData(DataFlow.All, Role.Multimedia)]
        public void SkipsOtherFlowOrRoles(DataFlow flow, Role role)
        {
            bool invoked = false;
            var notification = new AudioDeviceNotification(() => invoked = true);

            notification.OnDefaultDeviceChanged(flow, role, "id");

            Assert.False(invoked);
        }
    }
}
