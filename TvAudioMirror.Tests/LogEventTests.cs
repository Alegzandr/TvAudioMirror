using System.Collections.Generic;
using TvAudioMirror.Infrastructure.Logging;
using Xunit;

namespace TvAudioMirror.Tests
{
    public class LogEventTests
    {
        [Fact]
        public void DelegateSinkForwardsEvents()
        {
            var received = new List<LogEvent>();
            var sink = new DelegateLogSink(received.Add);
            var evt = LogEvent.Create(LogLevel.Info, "hello");

            sink.Publish(evt);

            Assert.Single(received);
            Assert.Equal(evt, received[0]);
        }

        [Fact]
        public void CreatePopulatesTimestamp()
        {
            var evt = LogEvent.Create(LogLevel.Warning, "something");
            Assert.Equal(LogLevel.Warning, evt.Level);
            Assert.Equal("something", evt.Message);
            Assert.NotEqual(default, evt.Timestamp);
        }
    }
}
