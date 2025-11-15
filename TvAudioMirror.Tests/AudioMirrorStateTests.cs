using TvAudioMirror.Core.Devices;
using TvAudioMirror.Core.Mirroring;
using Xunit;

namespace TvAudioMirror.Tests
{
    public class AudioMirrorStateTests
    {
        [Fact]
        public void MirroringStateContainsDefaultAndTvDevices()
        {
            var defaultDevice = new DeviceInfo("default", "Desktop Speakers");
            var tvDevice = new DeviceInfo("tv-1", "Living Room TV");

            var state = AudioMirrorState.Mirroring(defaultDevice, tvDevice);

            Assert.Equal(AudioMirrorStatus.Mirroring, state.Status);
            Assert.Equal(defaultDevice, state.DefaultDevice);
            Assert.Equal(tvDevice, state.TvDevice);
            Assert.Null(state.ErrorMessage);
        }

        [Fact]
        public void ErrorStateSupportsMissingDefaultDevice()
        {
            var state = AudioMirrorState.Error(null, "boom");

            Assert.Equal(AudioMirrorStatus.Error, state.Status);
            Assert.Null(state.DefaultDevice);
            Assert.Equal("boom", state.ErrorMessage);
        }
    }
}
