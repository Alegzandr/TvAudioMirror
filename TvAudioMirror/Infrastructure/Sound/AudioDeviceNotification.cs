using System;
using NAudio.CoreAudioApi;
using NAudio.CoreAudioApi.Interfaces;

namespace TvAudioMirror.Infrastructure.Sound
{
    internal sealed class AudioDeviceNotification : IMMNotificationClient
    {
        private readonly Action onDefaultChanged;

        public AudioDeviceNotification(Action onDefaultChanged)
        {
            this.onDefaultChanged = onDefaultChanged ?? throw new ArgumentNullException(nameof(onDefaultChanged));
        }

        public void OnDefaultDeviceChanged(DataFlow flow, Role role, string defaultDeviceId)
        {
            if (flow == DataFlow.Render && role == Role.Multimedia)
                onDefaultChanged();
        }

        public void OnDeviceAdded(string pwstrDeviceId) { }
        public void OnDeviceRemoved(string pwstrDeviceId) { }
        public void OnDeviceStateChanged(string pwstrDeviceId, DeviceState newState) { }
        public void OnPropertyValueChanged(string pwstrDeviceId, PropertyKey key) { }
    }
}
