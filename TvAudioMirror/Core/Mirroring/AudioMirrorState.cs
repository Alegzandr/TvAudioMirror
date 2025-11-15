using TvAudioMirror.Core.Devices;

namespace TvAudioMirror.Core.Mirroring
{
    internal enum AudioMirrorStatus
    {
        Idle,
        Monitoring,
        Mirroring,
        DefaultIsTv,
        TvNotFound,
        Error
    }

    internal readonly record struct AudioMirrorState(
        AudioMirrorStatus Status,
        DeviceInfo? DefaultDevice,
        DeviceInfo? TvDevice,
        string? ErrorMessage)
    {
        public static AudioMirrorState Idle() => new(AudioMirrorStatus.Idle, null, null, null);

        public static AudioMirrorState Monitoring(DeviceInfo defaultDevice) =>
            new(AudioMirrorStatus.Monitoring, defaultDevice, null, null);

        public static AudioMirrorState DefaultIsTv(DeviceInfo defaultDevice) =>
            new(AudioMirrorStatus.DefaultIsTv, defaultDevice, null, null);

        public static AudioMirrorState TvNotFound(DeviceInfo defaultDevice) =>
            new(AudioMirrorStatus.TvNotFound, defaultDevice, null, null);

        public static AudioMirrorState Mirroring(DeviceInfo defaultDevice, DeviceInfo tvDevice) =>
            new(AudioMirrorStatus.Mirroring, defaultDevice, tvDevice, null);

        public static AudioMirrorState Error(DeviceInfo? defaultDevice, string error) =>
            new(AudioMirrorStatus.Error, defaultDevice, null, error);
    }
}
