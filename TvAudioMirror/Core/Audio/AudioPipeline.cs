using System;
using NAudio.CoreAudioApi;
using NAudio.Wave;

namespace TvAudioMirror.Core.Audio
{
    internal interface IAudioPipeline : IDisposable
    {
        void Start(MMDevice source, MMDevice target);
        void Stop();
    }

    internal sealed class AudioPipeline : IAudioPipeline
    {
        private readonly Action<string> log;
        private readonly Func<MMDevice, WasapiLoopbackCapture> captureFactory;
        private readonly Func<MMDevice, AudioClientShareMode, bool, int, WasapiOut> tvOutputFactory;

        private WasapiLoopbackCapture? capture;
        private WasapiOut? tvOut;
        private BufferedWaveProvider? buffer;

        public AudioPipeline(
            Action<string> log,
            Func<MMDevice, WasapiLoopbackCapture>? captureFactory = null,
            Func<MMDevice, AudioClientShareMode, bool, int, WasapiOut>? tvOutputFactory = null)
        {
            this.log = log ?? (_ => { });
            this.captureFactory = captureFactory ?? (device => new WasapiLoopbackCapture(device));
            this.tvOutputFactory = tvOutputFactory ?? ((device, shareMode, useEventSync, latencyMs) =>
                new WasapiOut(device, shareMode, useEventSync, latencyMs));
        }

        public void Start(MMDevice source, MMDevice target)
        {
            if (source == null) throw new ArgumentNullException(nameof(source));
            if (target == null) throw new ArgumentNullException(nameof(target));

            Stop();

            capture = captureFactory(source);
            buffer = new BufferedWaveProvider(capture.WaveFormat)
            {
                DiscardOnBufferOverflow = true,
                BufferDuration = TimeSpan.FromMilliseconds(100)
            };

            tvOut = CreateTvOutput(target, buffer);

            capture.DataAvailable += (_, a) =>
            {
                buffer!.AddSamples(a.Buffer, 0, a.BytesRecorded);
            };

            capture.RecordingStopped += (_, _) =>
            {
                try { tvOut?.Stop(); } catch { }
            };

            capture.StartRecording();
            tvOut.Play();
        }

        private WasapiOut CreateTvOutput(MMDevice tv, IWaveProvider provider)
        {
            WasapiOut? exclusive = null;
            try
            {
                exclusive = tvOutputFactory(tv, AudioClientShareMode.Exclusive, true, 40);
                exclusive.Init(provider);
                log("Using exclusive audio mode for TV output.");
                return exclusive;
            }
            catch (Exception ex)
            {
                try { exclusive?.Dispose(); } catch { }
                log("Exclusive audio mode unavailable, falling back to shared mode. " + ex.Message);
            }

            var shared = tvOutputFactory(tv, AudioClientShareMode.Shared, true, 60);
            shared.Init(provider);
            log("Using shared audio mode for TV output.");
            return shared;
        }

        public void Stop()
        {
            try { capture?.StopRecording(); } catch { }
            try { capture?.Dispose(); } catch { }
            capture = null;

            try { tvOut?.Stop(); } catch { }
            try { tvOut?.Dispose(); } catch { }
            tvOut = null;

            buffer = null;
        }

        public void Dispose()
        {
            Stop();
        }
    }
}
