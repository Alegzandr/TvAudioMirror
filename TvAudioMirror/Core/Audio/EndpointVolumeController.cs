using System;

namespace TvAudioMirror.Core.Audio
{
    internal static class EndpointVolumeController
    {
        public static bool ToggleMute(IEndpointVolume volume)
        {
            if (volume == null) throw new ArgumentNullException(nameof(volume));
            volume.Mute = !volume.Mute;
            return volume.Mute;
        }

        public static int SetVolume(IEndpointVolume volume, float value)
        {
            if (volume == null) throw new ArgumentNullException(nameof(volume));
            var clamped = Math.Clamp(value, 0f, 1f);
            volume.MasterVolumeLevelScalar = clamped;
            return (int)Math.Round(clamped * 100, MidpointRounding.AwayFromZero);
        }
    }
}
