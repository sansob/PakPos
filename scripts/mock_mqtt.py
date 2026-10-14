#!/usr/bin/env python3
"""Tiny MQTT 3.1.1 broker for trying PakPos offline (and the screenshots).

    python3 scripts/mock_mqtt.py [port]     # default 1883

QoS 0 and 1, retained messages, + and # wildcards. Publishes a fake
temperature to kantor/<n>/suhu every 2 seconds. Not for real use.
"""
import random
import socket
import struct
import sys
import threading
import time

clients = {}  # sock -> [(filter, qos)]
retained = {}
lock = threading.Lock()


def varint(n):
    out = b""
    while True:
        b, n = n % 128, n // 128
        out += bytes([b | (0x80 if n else 0)])
        if not n:
            return out


def packet(first, body):
    return bytes([first]) + varint(len(body)) + body


def mqtt_str(s):
    b = s.encode()
    return struct.pack(">H", len(b)) + b


def matches(flt, topic):
    f, t = flt.split("/"), topic.split("/")
    for i, part in enumerate(f):
        if part == "#":
            return True
        if i >= len(t) or (part != "+" and part != t[i]):
            return False
    return len(f) == len(t)


def deliver(topic, payload, retain=False, skip=None):
    with lock:
        targets = [(s, q) for s, subs in clients.items() for f, q in subs if matches(f, topic) and s is not skip]
    for s, _ in targets:
        try:
            s.sendall(packet(0x30 | (1 if retain else 0), mqtt_str(topic) + payload))
        except OSError:
            pass


def read_packet(sock):
    first = sock.recv(1)
    if not first:
        return None
    n, mult = 0, 1
    while True:
        b = sock.recv(1)[0]
        n += (b & 0x7F) * mult
        mult *= 128
        if not b & 0x80:
            break
    body = b""
    while len(body) < n:
        chunk = sock.recv(n - len(body))
        if not chunk:
            return None
        body += chunk
    return first[0], body


def handle(sock):
    try:
        while True:
            p = read_packet(sock)
            if p is None:
                break
            first, body = p
            kind = first >> 4
            if kind == 1:  # CONNECT
                sock.sendall(packet(0x20, b"\x00\x00"))
                with lock:
                    clients[sock] = []
            elif kind == 3:  # PUBLISH
                qos, retain = (first >> 1) & 3, first & 1
                tlen = struct.unpack(">H", body[:2])[0]
                topic = body[2 : 2 + tlen].decode()
                at = 2 + tlen
                if qos:
                    pid = body[at : at + 2]
                    at += 2
                    sock.sendall(packet(0x40 if qos == 1 else 0x50, pid))
                payload = body[at:]
                if retain:
                    retained[topic] = payload
                deliver(topic, payload)
            elif kind == 6:  # PUBREL
                sock.sendall(packet(0x70, body[:2]))
            elif kind == 8:  # SUBSCRIBE
                pid, at, codes, new = body[:2], 2, b"", []
                while at < len(body):
                    tlen = struct.unpack(">H", body[at : at + 2])[0]
                    flt = body[at + 2 : at + 2 + tlen].decode()
                    qos = min(body[at + 2 + tlen], 1)
                    new.append((flt, qos))
                    codes += bytes([qos])
                    at += 3 + tlen
                with lock:
                    clients.setdefault(sock, []).extend(new)
                sock.sendall(packet(0x90, pid + codes))
                for topic, payload in list(retained.items()):
                    if any(matches(f, topic) for f, _ in new):
                        sock.sendall(packet(0x31, mqtt_str(topic) + payload))
            elif kind == 10:  # UNSUBSCRIBE
                sock.sendall(packet(0xB0, body[:2]))
            elif kind == 12:  # PINGREQ
                sock.sendall(b"\xd0\x00")
            elif kind == 14:  # DISCONNECT
                break
    except (OSError, IndexError):
        pass
    with lock:
        clients.pop(sock, None)
    sock.close()


def sensors():
    n = 0
    while True:
        time.sleep(2)
        n += 1
        for office in (1, 2):
            temp = round(26 + random.random() * 4, 1)
            deliver(f"kantor/{office}/suhu", f'{{"celsius": {temp}, "n": {n}}}'.encode())


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 1883
    retained["kantor/status"] = b'{"open": true, "since": "08:00"}'
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", port))
    srv.listen()
    threading.Thread(target=sensors, daemon=True).start()
    print(f"mock MQTT broker on mqtt://127.0.0.1:{port}")
    while True:
        conn, _ = srv.accept()
        threading.Thread(target=handle, args=(conn,), daemon=True).start()
