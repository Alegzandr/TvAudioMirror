using TvAudioMirror.Core.Audio;
using Xunit;

namespace TvAudioMirror.Tests
{
    public class EndpointVolumeControllerTests
    {
        private sealed class FakeVolume : IEndpointVolume
        {
            public bool Mute { get; set; }
            public float MasterVolumeLevelScalar { get; set; }
        }

        [Fact]
        public void ToggleMute_FlipsState()
        {
            var volume = new FakeVolume { Mute = false };

            var result = EndpointVolumeController.ToggleMute(volume);

            Assert.True(result);
            Assert.True(volume.Mute);
        }

        [Theory]
        [InlineData(-1f, 0, 0f)]
        [InlineData(0f, 0, 0f)]
        [InlineData(0.42f, 42, 0.42f)]
        [InlineData(0.425f, 43, 0.425f)]
        [InlineData(1.5f, 100, 1f)]
        public void SetVolume_ClampsAndReturnsPercent(float input, int expectedPercent, float expectedScalar)
        {
            var volume = new FakeVolume();

            var percent = EndpointVolumeController.SetVolume(volume, input);

            Assert.Equal(expectedPercent, percent);
            Assert.Equal(expectedScalar, volume.MasterVolumeLevelScalar, 3);
        }
    }
}
