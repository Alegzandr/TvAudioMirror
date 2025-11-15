using NAudio.CoreAudioApi;
using NAudio.CoreAudioApi.Interfaces;

namespace TvAudioMirror.Core.Devices
{
    internal interface IAudioDeviceCatalog
    {
        MMDevice GetDefaultRender();
        MMDevice? FindTv();
        void RegisterNotification(IMMNotificationClient client);
        void UnregisterNotification(IMMNotificationClient client);
    }

    internal sealed class WasapiDeviceCatalog : IAudioDeviceCatalog
    {
        private readonly MMDeviceEnumerator enumerator = new();

        public MMDevice GetDefaultRender() =>
            enumerator.GetDefaultAudioEndpoint(DataFlow.Render, Role.Multimedia);

        public MMDevice? FindTv()
        {
            var devs = enumerator.EnumerateAudioEndPoints(DataFlow.Render, DeviceState.Active);
            return DeviceSelector.FindFirstTv(devs, d => d.FriendlyName);
        }

        public void RegisterNotification(IMMNotificationClient client) =>
            enumerator.RegisterEndpointNotificationCallback(client);

        public void UnregisterNotification(IMMNotificationClient client)
        {
            try
            {
                enumerator.UnregisterEndpointNotificationCallback(client);
            }
            catch
            {
                // best effort
            }
        }
    }
}
