using System;

namespace TvAudioMirror.Infrastructure.Logging
{
    internal enum LogLevel
    {
        Debug,
        Info,
        Warning,
        Error
    }

    internal readonly record struct LogEvent(LogLevel Level, string Message, DateTime Timestamp)
    {
        public static LogEvent Create(LogLevel level, string message) =>
            new(level, message, DateTime.Now);
    }

    internal interface ILogSink
    {
        void Publish(LogEvent logEvent);
    }

    internal sealed class DelegateLogSink : ILogSink
    {
        private readonly Action<LogEvent> handler;

        public DelegateLogSink(Action<LogEvent> handler)
        {
            this.handler = handler ?? throw new ArgumentNullException(nameof(handler));
        }

        public void Publish(LogEvent logEvent) => handler(logEvent);
    }
}
