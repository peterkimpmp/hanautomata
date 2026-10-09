using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace Hanautomata
{
    internal static class Native
    {
        internal static readonly UIntPtr OutputTag = new UIntPtr(0x48464f55);
        internal static readonly UIntPtr TestTag = new UIntPtr(0x48465453);
        internal static readonly UIntPtr ProbeTag = new UIntPtr(0x48465052);
        internal const ushort ProbeKey = 0xe8; // Unassigned VK; key-up only, no character or modifier state.
        internal static bool BypassKeyboard(KeyboardData input)
        {
            // Remote keyboards can send either virtual keys or VK_PACKET Unicode characters.
            // Our own output and lower-integrity injections bypass; both input paths check field safety.
            return input.Extra == OutputTag || (input.Flags & 0x02) != 0;
        }
        internal delegate IntPtr HookProc(int code, IntPtr message, IntPtr data);
        internal delegate void WinEventProc(IntPtr hook, uint type, IntPtr hwnd, int obj, int child, uint thread, uint time);
        [StructLayout(LayoutKind.Sequential)] internal struct Point { public int X, Y; }
        [StructLayout(LayoutKind.Sequential)] internal struct Rect { public int Left, Top, Right, Bottom; }
        [StructLayout(LayoutKind.Sequential)] internal struct GuiInfo
        {
            public int Size, Flags;
            public IntPtr Active, Focus, Capture, MenuOwner, MoveSize, Caret;
            public Rect CaretRect;
        }
        [StructLayout(LayoutKind.Sequential)] internal struct KeyboardData
        { public uint Key, Scan, Flags, Time; public UIntPtr Extra; }
        [StructLayout(LayoutKind.Sequential)] internal struct KeyboardInput
        { public ushort Key, Scan; public uint Flags, Time; public UIntPtr Extra; }
        [StructLayout(LayoutKind.Sequential)] internal struct MouseInput
        { public int X, Y; public uint Data, Flags, Time; public UIntPtr Extra; }
        [StructLayout(LayoutKind.Explicit)] internal struct InputUnion
        { [FieldOffset(0)] public KeyboardInput Keyboard; [FieldOffset(0)] public MouseInput Mouse; }
        [StructLayout(LayoutKind.Sequential)] internal struct Input
        { public uint Type; public InputUnion Value; }
        [StructLayout(LayoutKind.Sequential)] internal struct Message
        { public IntPtr Window; public uint Id; public IntPtr WParam, LParam; public uint Time; public Point Location; }

        [DllImport("user32.dll", SetLastError = true)] internal static extern IntPtr SetWindowsHookEx(int id, HookProc callback, IntPtr module, uint thread);
        [DllImport("user32.dll")] internal static extern bool UnhookWindowsHookEx(IntPtr hook);
        [DllImport("user32.dll")] internal static extern IntPtr CallNextHookEx(IntPtr hook, int code, IntPtr message, IntPtr data);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] internal static extern IntPtr GetModuleHandle(string module);
        [DllImport("user32.dll", SetLastError = true)] internal static extern uint SendInput(uint count, Input[] inputs, int size);
        [DllImport("user32.dll")] internal static extern IntPtr GetForegroundWindow();
        [DllImport("user32.dll")] internal static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint process);
        [DllImport("user32.dll")] internal static extern bool GetGUIThreadInfo(uint thread, ref GuiInfo info);
        [DllImport("user32.dll")] internal static extern bool ClientToScreen(IntPtr hwnd, ref Point point);
        [DllImport("user32.dll")] internal static extern short GetAsyncKeyState(int key);
        [DllImport("user32.dll")] internal static extern short GetKeyState(int key);
        [DllImport("user32.dll")] internal static extern IntPtr SetWinEventHook(uint min, uint max, IntPtr module, WinEventProc callback, uint process, uint thread, uint flags);
        [DllImport("user32.dll")] internal static extern bool UnhookWinEvent(IntPtr hook);
        [DllImport("user32.dll")] internal static extern bool SetForegroundWindow(IntPtr hwnd);
        [DllImport("user32.dll")] internal static extern bool ShowWindow(IntPtr hwnd, int command);
        [DllImport("kernel32.dll")] internal static extern uint GetCurrentThreadId();
        [DllImport("user32.dll")] internal static extern int GetMessage(out Message message, IntPtr window, uint minimum, uint maximum);
        [DllImport("user32.dll")] internal static extern bool TranslateMessage(ref Message message);
        [DllImport("user32.dll")] internal static extern IntPtr DispatchMessage(ref Message message);
        [DllImport("user32.dll", SetLastError = true)] internal static extern bool PostThreadMessage(uint thread, uint message, IntPtr wParam, IntPtr lParam);
        [DllImport("user32.dll")] internal static extern bool AttachThreadInput(uint from, uint to, bool attach);
        [DllImport("user32.dll")] internal static extern bool BringWindowToTop(IntPtr hwnd);
        [DllImport("user32.dll")] internal static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int width, int height, uint flags);
        [DllImport("user32.dll", CharSet = CharSet.Unicode)] internal static extern IntPtr LoadKeyboardLayout(string id, uint flags);
        [DllImport("user32.dll")] internal static extern IntPtr GetKeyboardLayout(uint thread);
        [DllImport("user32.dll")] internal static extern IntPtr ActivateKeyboardLayout(IntPtr layout, uint flags);
        [DllImport("imm32.dll")] internal static extern IntPtr ImmGetContext(IntPtr hwnd);
        [DllImport("imm32.dll")] internal static extern bool ImmReleaseContext(IntPtr hwnd, IntPtr context);
        [DllImport("imm32.dll")] internal static extern bool ImmSetOpenStatus(IntPtr context, bool open);
        [DllImport("imm32.dll")] internal static extern bool ImmGetOpenStatus(IntPtr context);
        [DllImport("kernel32.dll", SetLastError = true)] internal static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
        [DllImport("kernel32.dll")] internal static extern bool CloseHandle(IntPtr handle);
        [DllImport("advapi32.dll", SetLastError = true)] internal static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
        [DllImport("advapi32.dll", SetLastError = true)] internal static extern bool GetTokenInformation(IntPtr token, int cls, IntPtr info, int length, out int needed);
        [DllImport("advapi32.dll")] internal static extern IntPtr GetSidSubAuthorityCount(IntPtr sid);
        [DllImport("advapi32.dll")] internal static extern IntPtr GetSidSubAuthority(IntPtr sid, uint index);
        [DllImport("user32.dll")] internal static extern bool DestroyIcon(IntPtr icon);
        [DllImport("user32.dll")] internal static extern bool IsWindowVisible(IntPtr hwnd);
        [DllImport("user32.dll", CharSet = CharSet.Unicode)] internal static extern int GetClassName(IntPtr hwnd, StringBuilder name, int maximum);
        [DllImport("user32.dll", EntryPoint = "GetWindowLongW")] internal static extern int GetWindowStyle(IntPtr hwnd, int index);
        // Raw Input (sentinel for a silently removed low-level hook) and Win32 clipboard (selection flip fallback).
        [StructLayout(LayoutKind.Sequential)] internal struct RawInputDevice { public ushort UsagePage, Usage; public uint Flags; public IntPtr Target; }
        internal static readonly int RawHeaderSize = 8 + 2 * IntPtr.Size; // dwType, dwSize, hDevice, wParam
        [DllImport("user32.dll", SetLastError = true)] internal static extern bool RegisterRawInputDevices(RawInputDevice[] devices, uint count, uint size);
        [DllImport("user32.dll")] internal static extern uint GetRawInputData(IntPtr input, uint command, IntPtr data, ref uint size, uint headerSize);
        [DllImport("user32.dll", SetLastError = true)] internal static extern bool OpenClipboard(IntPtr owner);
        [DllImport("user32.dll")] internal static extern bool CloseClipboard();
        [DllImport("user32.dll")] internal static extern bool EmptyClipboard();
        [DllImport("user32.dll")] internal static extern IntPtr GetClipboardData(uint format);
        [DllImport("user32.dll")] internal static extern IntPtr SetClipboardData(uint format, IntPtr data);
        [DllImport("user32.dll")] internal static extern bool IsClipboardFormatAvailable(uint format);
        [DllImport("user32.dll")] internal static extern int CountClipboardFormats();
        [DllImport("user32.dll")] internal static extern uint GetClipboardSequenceNumber();
        [DllImport("kernel32.dll")] internal static extern IntPtr GlobalAlloc(uint flags, UIntPtr bytes);
        [DllImport("kernel32.dll")] internal static extern IntPtr GlobalLock(IntPtr handle);
        [DllImport("kernel32.dll")] internal static extern bool GlobalUnlock(IntPtr handle);
        [DllImport("kernel32.dll")] internal static extern IntPtr GlobalFree(IntPtr handle);
        static bool OpenClipboardWithRetry()
        {
            for (int attempt = 0; attempt < 8; attempt++) { if (OpenClipboard(IntPtr.Zero)) return true; System.Threading.Thread.Sleep(10); }
            return false;
        }
        // 0 = empty, 1 = text available, 2 = only non-text content (never overwritten by the selection fallback).
        internal static int ClipboardState()
        {
            if (!OpenClipboardWithRetry()) return 2;
            try { return CountClipboardFormats() == 0 ? 0 : IsClipboardFormatAvailable(13) ? 1 : 2; }
            finally { CloseClipboard(); }
        }
        internal static string ReadClipboardText()
        {
            if (!OpenClipboardWithRetry()) return null;
            try
            {
                if (!IsClipboardFormatAvailable(13)) return null;
                IntPtr handle = GetClipboardData(13); if (handle == IntPtr.Zero) return null;
                IntPtr pointer = GlobalLock(handle); if (pointer == IntPtr.Zero) return null;
                try { return Marshal.PtrToStringUni(pointer); } finally { GlobalUnlock(handle); }
            }
            finally { CloseClipboard(); }
        }
        internal static bool WriteClipboardText(string text)
        {
            if (!OpenClipboardWithRetry()) return false;
            try
            {
                EmptyClipboard();
                if (text == null) return true;
                int bytes = (text.Length + 1) * 2;
                IntPtr handle = GlobalAlloc(0x2042, new UIntPtr((uint)bytes)); // GMEM_MOVEABLE | GMEM_ZEROINIT
                if (handle == IntPtr.Zero) return false;
                IntPtr pointer = GlobalLock(handle);
                if (pointer == IntPtr.Zero) { GlobalFree(handle); return false; }
                Marshal.Copy(text.ToCharArray(), 0, pointer, text.Length); GlobalUnlock(handle);
                if (SetClipboardData(13, handle) == IntPtr.Zero) { GlobalFree(handle); return false; }
                return true;
            }
            finally { CloseClipboard(); }
        }

        internal static bool IsProtectedNativeEdit(IntPtr hwnd)
        {
            var name = new StringBuilder(128); GetClassName(hwnd, name, name.Capacity);
            string cls = name.ToString().ToUpperInvariant();
            if (cls != "EDIT" && !cls.Contains(".EDIT.") && !cls.StartsWith("RICHEDIT")) return false;
            return (GetWindowStyle(hwnd, -16) & (0x20 | 0x800)) != 0;
        }

        internal static GuiInfo GetFocus(IntPtr foreground)
        {
            var info = new GuiInfo { Size = Marshal.SizeOf(typeof(GuiInfo)) };
            uint pid; uint thread = GetWindowThreadProcessId(foreground, out pid);
            if (thread == 0 || !GetGUIThreadInfo(thread, ref info)) return new GuiInfo();
            return info;
        }
        internal static int IntegrityLevel(uint pid)
        {
            IntPtr process = OpenProcess(0x1000, false, pid), token = IntPtr.Zero, buffer = IntPtr.Zero;
            try
            {
                if (process == IntPtr.Zero || !OpenProcessToken(process, 8, out token)) return -1;
                int size; GetTokenInformation(token, 25, IntPtr.Zero, 0, out size);
                if (size <= 0 || size > 65536) return -1;
                buffer = Marshal.AllocHGlobal(size);
                if (!GetTokenInformation(token, 25, buffer, size, out size)) return -1;
                IntPtr sid = Marshal.ReadIntPtr(buffer);
                int count = Marshal.ReadByte(GetSidSubAuthorityCount(sid));
                return Marshal.ReadInt32(GetSidSubAuthority(sid, (uint)(count - 1)));
            }
            finally
            {
                if (buffer != IntPtr.Zero) Marshal.FreeHGlobal(buffer);
                if (token != IntPtr.Zero) CloseHandle(token);
                if (process != IntPtr.Zero) CloseHandle(process);
            }
        }
        internal static Input Key(ushort key, bool up, UIntPtr tag, bool extended)
        {
            return new Input { Type = 1, Value = new InputUnion { Keyboard = new KeyboardInput {
                Key = key, Flags = (up ? 2u : 0u) | (extended ? 1u : 0u), Extra = tag } } };
        }
        internal static List<Input> Unicode(string text)
        {
            var result = new List<Input>();
            foreach (char c in text)
            {
                result.Add(new Input { Type = 1, Value = new InputUnion { Keyboard = new KeyboardInput { Scan = c, Flags = 4, Extra = OutputTag } } });
                result.Add(new Input { Type = 1, Value = new InputUnion { Keyboard = new KeyboardInput { Scan = c, Flags = 6, Extra = OutputTag } } });
            }
            return result;
        }
        internal static bool Inject(List<Input> events)
        {
            if (events.Count == 0) return true;
            return SendInput((uint)events.Count, events.ToArray(), Marshal.SizeOf(typeof(Input))) == events.Count;
        }
        internal static bool IsDown(int key) { return (GetAsyncKeyState(key) & 0x8000) != 0; }
        internal static bool TryAscii(int vk, bool shift, bool caps, out char literal, out char hangul)
        {
            literal = hangul = '\0';
            if (vk >= 65 && vk <= 90)
            {
                literal = (char)(shift ^ caps ? vk : vk + 32);
                hangul = (char)(shift ? vk : vk + 32);
                return true;
            }
            if (vk >= 48 && vk <= 57) { literal = hangul = shift ? ")!@#$%^&*("[vk - 48] : (char)vk; return true; }
            const string plain = ";=,-./`[\\]'", shifted = ":+<_>?~{|}\"";
            int index = Array.IndexOf(new[] { 186, 187, 188, 189, 190, 191, 192, 219, 220, 221, 222 }, vk);
            if (index < 0) return false;
            literal = hangul = shift ? shifted[index] : plain[index]; return true;
        }
    }
}
