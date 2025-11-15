using NAudio.CoreAudioApi;

namespace TvAudioMirror.Core.Audio
{
    internal interface IEndpointVolume
    {
        bool Mute { get; set; }
        float MasterVolumeLevelScalar { get; set; }
    }

    internal sealed class AudioEndpointVolumeAdapter : IEndpointVolume
    {
        private readonly AudioEndpointVolume inner;

        public AudioEndpointVolumeAdapter(AudioEndpointVolume inner)
        {
            this.inner = inner;
        }

        public bool Mute
        {
            get => inner.Mute;
            set => inner.Mute = value;
        }

        public float MasterVolumeLevelScalar
        {
            get => inner.MasterVolumeLevelScalar;
            set => inner.MasterVolumeLevelScalar = value;
        }
    }
}
