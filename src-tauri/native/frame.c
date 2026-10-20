#define UNICODE
#define _UNICODE
#define WINVER 0x0A00
#define _WIN32_WINNT 0x0A00
#include <windows.h>
#include <windowsx.h>
#include <commctrl.h>

static const WCHAR active_property[] = L"ChromeThemer.Frame.Owner.v1";
static const WCHAR lease_property[] = L"ChromeThemer.Frame.Lease.v1";
static const WCHAR attach_message[] = L"ChromeThemer.Frame.Attach.v1";
static const WCHAR detach_message[] = L"ChromeThemer.Frame.Detach.v1";
static const UINT_PTR subclass_id = 0x43544631;

typedef struct {
    HANDLE lease;
    DWORD owner_thread;
    UINT_PTR timer;
} Frame;

static LRESULT CALLBACK frame_proc(HWND, UINT, WPARAM, LPARAM, UINT_PTR, DWORD_PTR);

static LRESULT CALLBACK child_proc(HWND window, UINT message, WPARAM wparam,
    LPARAM lparam, UINT_PTR id, DWORD_PTR reference) {
    HWND root = (HWND)reference;
    if (message == WM_NCHITTEST && GetPropW(root, active_property)) {
        LRESULT hit = SendMessageW(root, message, wparam, lparam);
        /* Renderer HWNDs cover the custom frame; pass only frame hits to the parent. */
        if (hit == HTCAPTION || (hit >= HTLEFT && hit <= HTBOTTOMRIGHT))
            return HTTRANSPARENT;
    }
    if (message == WM_NCDESTROY)
        RemoveWindowSubclass(window, child_proc, id);
    return DefSubclassProc(window, message, wparam, lparam);
}

typedef struct {
    HWND root;
    BOOL success;
} Children;

static BOOL CALLBACK attach_child(HWND window, LPARAM parameter) {
    Children *children = (Children *)parameter;
    if (GetWindowThreadProcessId(window, NULL) == GetCurrentThreadId()
        && !SetWindowSubclass(window, child_proc, subclass_id, (DWORD_PTR)children->root))
        children->success = FALSE;
    return TRUE;
}

static BOOL attach_children(HWND window) {
    Children children = {.root = window, .success = TRUE};
    EnumChildWindows(window, attach_child, (LPARAM)&children);
    return children.success;
}

static BOOL CALLBACK detach_child(HWND window, LPARAM root) {
    DWORD_PTR reference;
    if (GetWindowThreadProcessId(window, NULL) == GetCurrentThreadId()
        && GetWindowSubclass(window, child_proc, subclass_id, &reference)
        && reference == (DWORD_PTR)root)
        RemoveWindowSubclass(window, child_proc, subclass_id);
    return TRUE;
}

static void refresh(HWND window) {
    SetWindowPos(window, NULL, 0, 0, 0, 0,
        SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
}

static BOOL detach(HWND window, Frame *frame, BOOL destroying) {
    if (!RemoveWindowSubclass(window, frame_proc, subclass_id)) return FALSE;
    RemovePropW(window, active_property);
    EnumChildWindows(window, detach_child, (LPARAM)window);
    KillTimer(window, frame->timer);
    CloseHandle(frame->lease);
    HeapFree(GetProcessHeap(), 0, frame);
    if (!destroying) refresh(window);
    return TRUE;
}

static BOOL framed(HWND window) {
    return (GetWindowLongPtrW(window, GWL_STYLE) & WS_THICKFRAME) != 0;
}

static LRESULT edge_hit(HWND window, LPARAM position) {
    RECT bounds;
    GetWindowRect(window, &bounds);
    UINT dpi = GetDpiForWindow(window);
    int x_border = GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi)
        + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
    int y_border = GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi)
        + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
    int x = GET_X_LPARAM(position), y = GET_Y_LPARAM(position);
    BOOL left = x < bounds.left + x_border, right = x >= bounds.right - x_border;
    BOOL top = y < bounds.top + y_border, bottom = y >= bounds.bottom - y_border;
    if (top) return left ? HTTOPLEFT : right ? HTTOPRIGHT : HTTOP;
    if (bottom) return left ? HTBOTTOMLEFT : right ? HTBOTTOMRIGHT : HTBOTTOM;
    return left ? HTLEFT : right ? HTRIGHT : HTNOWHERE;
}

static LRESULT CALLBACK frame_proc(HWND window, UINT message, WPARAM wparam,
    LPARAM lparam, UINT_PTR id, DWORD_PTR reference) {
    (void)id;
    Frame *frame = (Frame *)reference;
    if (message >= 0xc000 && message == RegisterWindowMessageW(detach_message)
        && wparam == frame->owner_thread) {
        return detach(window, frame, FALSE);
    }
    switch (message) {
    case WM_TIMER:
        if (wparam == frame->timer) {
            if (WaitForSingleObject(frame->lease, 0) != WAIT_TIMEOUT || !attach_children(window))
                detach(window, frame, FALSE);
            return 0;
        }
        break;
    case WM_NCDESTROY:
        detach(window, frame, TRUE);
        break;
    case WM_NCLBUTTONDOWN:
        if (wparam == HTCAPTION || (wparam >= HTLEFT && wparam <= HTBOTTOMRIGHT)) {
            ReleaseCapture();
            return DefWindowProcW(window, message, wparam, lparam);
        }
        break;
    case WM_NCLBUTTONDBLCLK:
        if (wparam == HTCAPTION)
            return DefWindowProcW(window, message, wparam, lparam);
        break;
    case WM_NCCALCSIZE: {
        RECT *client = wparam ? &((NCCALCSIZE_PARAMS *)lparam)->rgrc[0] : (RECT *)lparam;
        if (IsZoomed(window) && framed(window)) {
            MONITORINFO monitor = {.cbSize = sizeof(MONITORINFO)};
            if (GetMonitorInfoW(MonitorFromRect(client, MONITOR_DEFAULTTONEAREST), &monitor))
                IntersectRect(client, client, &monitor.rcWork);
        }
        return 0;
    }
    case WM_GETMINMAXINFO: {
        LRESULT handled = DefSubclassProc(window, message, wparam, lparam);
        if (framed(window)) {
            MONITORINFO monitor = {.cbSize = sizeof(MONITORINFO)};
            if (GetMonitorInfoW(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST), &monitor)) {
                MINMAXINFO *limits = (MINMAXINFO *)lparam;
                limits->ptMaxPosition.x = monitor.rcWork.left - monitor.rcMonitor.left;
                limits->ptMaxPosition.y = monitor.rcWork.top - monitor.rcMonitor.top;
                limits->ptMaxSize.x = monitor.rcWork.right - monitor.rcWork.left;
                limits->ptMaxSize.y = monitor.rcWork.bottom - monitor.rcWork.top;
            }
        }
        return handled;
    }
    case WM_NCHITTEST:
        if (!IsZoomed(window) && framed(window)) {
            LRESULT hit = edge_hit(window, lparam);
            if (hit != HTNOWHERE) return hit;
        }
        {
            LRESULT hit = DefSubclassProc(window, message, wparam, lparam);
            if (hit == HTMINBUTTON || hit == HTMAXBUTTON || hit == HTCLOSE || hit == HTSYSMENU)
                return HTCLIENT;
            return hit;
        }
    }
    return DefSubclassProc(window, message, wparam, lparam);
}

static void attach(HWND window, DWORD owner_pid, DWORD owner_thread) {
    DWORD pid;
    DWORD thread = GetWindowThreadProcessId(window, &pid);
    WCHAR name[64];
    if (thread != GetCurrentThreadId() || pid != GetCurrentProcessId()
        || GetAncestor(window, GA_ROOT) != window || GetPropW(window, active_property)) return;
    if (!GetClassNameW(window, name, 64) || wcscmp(name, L"Chrome_WidgetWin_1")) return;
    Frame *frame = HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, sizeof(Frame));
    if (!frame) return;
    frame->owner_thread = owner_thread;
    frame->lease = OpenThread(SYNCHRONIZE | THREAD_QUERY_LIMITED_INFORMATION, FALSE, owner_thread);
    FILETIME created, exited, kernel, user;
    /* The subclass can outlive its bootstrap hook; pin its code until Chrome exits. */
    HMODULE module;
    if (!frame->lease || GetProcessIdOfThread(frame->lease) != owner_pid
        || WaitForSingleObject(frame->lease, 0) != WAIT_TIMEOUT
        || !GetThreadTimes(frame->lease, &created, &exited, &kernel, &user)
        || (((ULONGLONG)created.dwHighDateTime << 32) | created.dwLowDateTime)
            != (ULONGLONG)(UINT_PTR)GetPropW(window, lease_property)
        || !GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            (LPCWSTR)frame_proc, &module)) goto failed;
    if (!SetWindowSubclass(window, frame_proc, subclass_id, (DWORD_PTR)frame)) goto failed;
    frame->timer = SetTimer(window, (UINT_PTR)frame, 500, NULL);
    if (!frame->timer || !attach_children(window)
        || !SetPropW(window, active_property, (HANDLE)(UINT_PTR)owner_thread)) {
        if (!RemoveWindowSubclass(window, frame_proc, subclass_id)) return;
        EnumChildWindows(window, detach_child, (LPARAM)window);
        if (frame->timer) KillTimer(window, frame->timer);
        goto failed;
    }
    refresh(window);
    return;
failed:
    if (frame->lease) CloseHandle(frame->lease);
    HeapFree(GetProcessHeap(), 0, frame);
}

__declspec(dllexport) LRESULT CALLBACK FrameHook(int code, WPARAM wparam, LPARAM lparam) {
    if (code >= 0) {
        const CWPSTRUCT *message = (const CWPSTRUCT *)lparam;
        if (message->message == RegisterWindowMessageW(attach_message))
            attach(message->hwnd, (DWORD)message->wParam, (DWORD)message->lParam);
    }
    return CallNextHookEx(NULL, code, wparam, lparam);
}
