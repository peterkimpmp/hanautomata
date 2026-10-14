using System;
using System.IO;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Threading;

namespace Hanautomata
{
    internal static class ImeSharedAccess
    {
        // Called only for validated, newly written settings bytes, never for the previous/corrupt backup.
        internal static void GrantSettingsRead(string path)
        {
            if ((File.GetAttributes(path) & (FileAttributes.Directory | FileAttributes.ReparsePoint)) != 0)
                throw new IOException("설정 읽기 권한은 일반 파일에만 부여할 수 있습니다.");
            var security = File.GetAccessControl(path, AccessControlSections.Access);
            security.AddAccessRule(new FileSystemAccessRule(new SecurityIdentifier("S-1-15-2-1"),
                FileSystemRights.Read | FileSystemRights.Synchronize, AccessControlType.Allow));
            File.SetAccessControl(path, security);
        }
        internal static Mutex CreateTrayMutex(string name, out bool created)
        {
            var security = new MutexSecurity();
            using (var identity = WindowsIdentity.GetCurrent())
                security.AddAccessRule(new MutexAccessRule(identity.User, MutexRights.FullControl, AccessControlType.Allow));
            security.AddAccessRule(new MutexAccessRule(new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null), MutexRights.FullControl, AccessControlType.Allow));
            security.AddAccessRule(new MutexAccessRule(new SecurityIdentifier(WellKnownSidType.BuiltinAdministratorsSid, null), MutexRights.FullControl, AccessControlType.Allow));
            security.AddAccessRule(new MutexAccessRule(new SecurityIdentifier("S-1-15-2-1"), MutexRights.Synchronize, AccessControlType.Allow));
            return new Mutex(true, name, out created, security);
        }
    }
}
