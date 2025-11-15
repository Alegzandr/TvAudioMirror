using System;
using System.Collections.Generic;
using TvAudioMirror.Core.Devices;
using Xunit;

namespace TvAudioMirror.Tests
{
    public class DeviceSelectorTests
    {
        private sealed record FakeDevice(string FriendlyName);

        [Theory]
        [InlineData("Living Room TV")]
        [InlineData("desktop tv speakers")]
        [InlineData("tV output (HDMI)")]
        [InlineData("MyTV")]
        public void IsTvName_DetectsTvSubstrings(string name)
        {
            Assert.True(DeviceSelector.IsTvName(name));
        }

        [Theory]
        [InlineData("")]
        [InlineData("  ")]
        [InlineData(null)]
        [InlineData("Television")]
        [InlineData("Monitor")]
        public void IsTvName_IgnoresNonMatches(string? name)
        {
            Assert.False(DeviceSelector.IsTvName(name));
        }

        [Fact]
        public void FindFirstTv_ReturnsFirstMatch()
        {
            var devices = new[]
            {
                new FakeDevice("Speakers"),
                new FakeDevice("Bedroom tv"),
                new FakeDevice("Living Room TV")
            };

            var selected = DeviceSelector.FindFirstTv(devices, d => d.FriendlyName);

            Assert.Same(devices[1], selected);
        }

        [Fact]
        public void FindFirstTv_ReturnsNullWhenMissing()
        {
            var devices = new[]
            {
                new FakeDevice("Speakers"),
                new FakeDevice("Soundbar"),
                new FakeDevice("Headphones")
            };

            var selected = DeviceSelector.FindFirstTv(devices, d => d.FriendlyName);

            Assert.Null(selected);
        }

        [Fact]
        public void FindFirstTv_ThrowsWhenDevicesNull()
        {
            Assert.Throws<ArgumentNullException>(() => DeviceSelector.FindFirstTv<FakeDevice>(null!, d => d.FriendlyName));
        }

        [Fact]
        public void FindFirstTv_ThrowsWhenAccessorNull()
        {
            var devices = new List<FakeDevice>();
            Assert.Throws<ArgumentNullException>(() => DeviceSelector.FindFirstTv(devices, null!));
        }
    }
}
