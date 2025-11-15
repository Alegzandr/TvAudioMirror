using NAudio.CoreAudioApi;

namespace TvAudioMirror.Core.Devices
{
    internal readonly record struct DeviceInfo(string Id, string FriendlyName)
    {
        public bool IsTv => DeviceSelector.IsTvName(FriendlyName);

        public static DeviceInfo FromMmDevice(MMDevice device) => new(device.ID, device.FriendlyName);
    }

    internal enum RefreshOutcome
    {
        DefaultIsTv,
        TvNotFound,
        StartPipeline
    }

    internal readonly record struct DeviceRefreshDecision(RefreshOutcome Outcome, DeviceInfo DefaultDevice, DeviceInfo? TvDevice);

    internal static class DeviceRefreshDecider
    {
        public static DeviceRefreshDecision Decide(DeviceInfo defaultDevice, DeviceInfo? tvDevice)
        {
            if (defaultDevice.IsTv)
                return new DeviceRefreshDecision(RefreshOutcome.DefaultIsTv, defaultDevice, null);

            if (tvDevice == null)
                return new DeviceRefreshDecision(RefreshOutcome.TvNotFound, defaultDevice, null);

            return new DeviceRefreshDecision(RefreshOutcome.StartPipeline, defaultDevice, tvDevice.Value);
        }
    }
}
