import ctypes
from ctypes import wintypes
import os
import signal
import time


class Processes:
    def __init__(self):
        self.processes = []
        self.handle = None
        if os.name != 'nt':
            return

        class Limits(ctypes.Structure):
            _fields_ = [('process_time', ctypes.c_int64), ('job_time', ctypes.c_int64),
                        ('flags', wintypes.DWORD), ('minimum', ctypes.c_size_t),
                        ('maximum', ctypes.c_size_t), ('active_limit', wintypes.DWORD),
                        ('affinity', ctypes.c_size_t), ('priority', wintypes.DWORD),
                        ('scheduling', wintypes.DWORD)]

        class Extended(ctypes.Structure):
            _fields_ = [('limits', Limits), ('io', ctypes.c_uint64 * 6),
                        ('process_memory', ctypes.c_size_t), ('job_memory', ctypes.c_size_t),
                        ('peak_process', ctypes.c_size_t), ('peak_job', ctypes.c_size_t)]

        self.kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        self.kernel.CreateJobObjectW.restype = wintypes.HANDLE
        self.kernel.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        self.kernel.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int,
                                                       ctypes.c_void_p, wintypes.DWORD]
        self.kernel.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        self.kernel.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
        self.kernel.QueryInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int,
                                                         ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p]
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.handle = self.kernel.CreateJobObjectW(None, None)
        if not self.handle:
            raise ctypes.WinError(ctypes.get_last_error())
        info = Extended()
        info.limits.flags = 0x2000
        if not self.kernel.SetInformationJobObject(self.handle, 9, ctypes.byref(info), ctypes.sizeof(info)):
            self.kernel.CloseHandle(self.handle)
            self.handle = None
            raise ctypes.WinError(ctypes.get_last_error())

    def add(self, process):
        self.processes.append(process)
        if self.handle and not self.kernel.AssignProcessToJobObject(self.handle, wintypes.HANDLE(process._handle)):
            process.kill()
            process.wait(timeout=10)
            raise ctypes.WinError(ctypes.get_last_error())

    def close(self):
        if self.handle:
            if not self.kernel.TerminateJobObject(self.handle, 1):
                raise ctypes.WinError(ctypes.get_last_error())
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                accounting = (ctypes.c_uint64 * 4)()
                counts = (wintypes.DWORD * 4)()

                class Accounting(ctypes.Structure):
                    _fields_ = [('times', ctypes.c_uint64 * 4), ('counts', wintypes.DWORD * 4)]

                result = Accounting(accounting, counts)
                if not self.kernel.QueryInformationJobObject(self.handle, 1, ctypes.byref(result), ctypes.sizeof(result), None):
                    raise ctypes.WinError(ctypes.get_last_error())
                if result.counts[2] == 0:
                    break
                time.sleep(.1)
            else:
                raise TimeoutError('owned Windows process tree remains active')
            self.kernel.CloseHandle(self.handle)
            self.handle = None
        else:
            for process in self.processes:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and any(process.poll() is None for process in self.processes):
                time.sleep(.1)
            for process in self.processes:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        for process in self.processes:
            process.wait(timeout=10)
        return all(process.poll() is not None for process in self.processes)
