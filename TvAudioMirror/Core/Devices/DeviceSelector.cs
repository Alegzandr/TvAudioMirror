using System;
using System.Collections.Generic;

namespace TvAudioMirror.Core.Devices
{
    internal static class DeviceSelector
    {
        public static bool IsTvName(string? friendlyName)
        {
            if (string.IsNullOrWhiteSpace(friendlyName))
                return false;

            return friendlyName.IndexOf("tv", StringComparison.OrdinalIgnoreCase) >= 0;
        }

        public static T? FindFirstTv<T>(IEnumerable<T> devices, Func<T, string?> friendlyNameAccessor) where T : class
        {
            if (devices == null) throw new ArgumentNullException(nameof(devices));
            if (friendlyNameAccessor == null) throw new ArgumentNullException(nameof(friendlyNameAccessor));

            foreach (var device in devices)
            {
                var name = friendlyNameAccessor(device);
                if (IsTvName(name))
                    return device;
            }

            return null;
        }
    }
}
