"""Shared bits of the chain test's local servers (CI only): a JSON-lines log
and, on Windows, which process opened a loopback connection (GetExtendedTcpTable)."""
import ctypes
import json
import os
import socket
import sys
import threading
from datetime import datetime, timezone

_lock = threading.Lock()


def stamp():
    return datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%S.%f')[:-3] + 'Z'


class Log:
    def __init__(self, path):
        self.path = path

    def write(self, **fields):
        line = json.dumps({'t': stamp(), **fields}, ensure_ascii=False)
        with _lock:
            with open(self.path, 'a', encoding='utf-8') as f:
                f.write(line + '\n')
        print(line, flush=True)


def _image_name(pid):
    kernel32 = ctypes.windll.kernel32
    handle = kernel32.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
    if not handle:
        return None
    try:
        size = ctypes.c_uint32(1024)
        buf = ctypes.create_unicode_buffer(1024)
        if kernel32.QueryFullProcessImageNameW(handle, 0, buf, ctypes.byref(size)):
            return os.path.basename(buf.value)
        return None
    finally:
        kernel32.CloseHandle(handle)


def owner(peer, server):
    """(pid, image name) of the process on the client side of a loopback IPv4 connection."""
    if sys.platform != 'win32':
        return None, None

    class Row(ctypes.Structure):
        _fields_ = [('state', ctypes.c_uint32), ('local_addr', ctypes.c_uint32), ('local_port', ctypes.c_uint32),
                    ('remote_addr', ctypes.c_uint32), ('remote_port', ctypes.c_uint32), ('pid', ctypes.c_uint32)]

    iphlpapi = ctypes.windll.iphlpapi
    size = ctypes.c_uint32(0)
    iphlpapi.GetExtendedTcpTable(None, ctypes.byref(size), False, 2, 5, 0)  # AF_INET, TCP_TABLE_OWNER_PID_ALL
    for _ in range(3):
        buf = ctypes.create_string_buffer(size.value + 4096)
        size = ctypes.c_uint32(len(buf))
        if iphlpapi.GetExtendedTcpTable(buf, ctypes.byref(size), False, 2, 5, 0) == 0:
            break
    else:
        return None, None
    count = ctypes.c_uint32.from_buffer(buf).value
    rows = (Row * count).from_buffer(buf, 4)
    peer_port, server_port = peer[1], server[1]
    for row in rows:
        if socket.ntohs(row.local_port & 0xFFFF) == peer_port and socket.ntohs(row.remote_port & 0xFFFF) == server_port:
            return row.pid, _image_name(row.pid)
    return None, None
