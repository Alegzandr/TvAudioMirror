using TvAudioMirror.Core.Devices;
using Xunit;

namespace TvAudioMirror.Tests
{
    public class DeviceRefreshDecisionTests
    {
        private static DeviceInfo Device(string id, string name) => new(id, name);

        [Fact]
        public void MirrorsDefaultDeviceToTvWhenAvailable()
        {
            var def = Device("default", "Speakers");
            var tv = Device("tv-1", "Living Room TV");

            var decision = DeviceRefreshDecider.Decide(def, tv);

            Assert.Equal(RefreshOutcome.StartPipeline, decision.Outcome);
            Assert.Equal(tv, decision.TvDevice);
        }

        [Fact]
        public void SkipsMirroringWhenDefaultIsAlreadyTv()
        {
            var def = Device("default", "Samsung TV");

            var decision = DeviceRefreshDecider.Decide(def, null);

            Assert.Equal(RefreshOutcome.DefaultIsTv, decision.Outcome);
            Assert.Null(decision.TvDevice);
        }

        [Fact]
        public void ReportsWhenNoTvFound()
        {
            var def = Device("default", "Desktop Speakers");

            var decision = DeviceRefreshDecider.Decide(def, null);

            Assert.Equal(RefreshOutcome.TvNotFound, decision.Outcome);
            Assert.Null(decision.TvDevice);
        }
    }
}
