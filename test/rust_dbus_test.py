#!/usr/bin/env python3
"""
OpenBMC Rust D-Bus Regression Test
테스트 항목:
  1. Property 읽기 (MACAddress)
  2. Property 쓰기 (Set)
  3. PropertiesChanged 시그널 수신
  4. ObjectMapper 객체 등록 확인
  5. 서비스 재시작 후 값 유지
"""

import dbus
import dbus.mainloop.glib
from gi.repository import GLib
import subprocess
import time
import sys

# ── 설정 ─────────────────────────────────────────────────────
SERVICE      = "xyz.openbmc_project.Network"
OBJECT_PATH  = "/xyz/openbmc_project/network/enp5s0"
INTERFACE    = "xyz.openbmc_project.Network.MACAddress"  
MAPPER_SVC   = "xyz.openbmc_project.ObjectMapper"
MAPPER_PATH  = "/xyz/openbmc_project/object_mapper"
MAPPER_IFACE = "xyz.openbmc_project.ObjectMapper"

PASS = "\033[92m[PASS]\033[0m"
FAIL = "\033[91m[FAIL]\033[0m"
INFO = "\033[94m[INFO]\033[0m"

results = []

def record(name, ok, detail=""):
    results.append((name, ok, detail))
    status = PASS if ok else FAIL
    print(f"{status} {name}" + (f" : {detail}" if detail else ""))

# ── 1. Property 읽기 ─────────────────────────────────────────
def test_get_property(bus):
    print(f"\n{INFO} 1. Property 읽기 테스트")
    try:
        obj = bus.get_object(SERVICE, OBJECT_PATH)
        props = dbus.Interface(obj, "org.freedesktop.DBus.Properties")
        #mac = props.Get(INTERFACE, "MacAddress")   ##Lighton: this is for qemu
        mac = props.Get(INTERFACE, "MacAddress")    ##         this is for ubuntu
        ok = isinstance(mac, str) and len(mac) > 0
        record("Property 읽기", ok, f"MACAddress = {mac}")
        return str(mac)
    except Exception as e:
        record("Property 읽기", False, str(e))
        return None

# ── 2. Property 쓰기 ─────────────────────────────────────────
def test_set_property(bus):
    print(f"\n{INFO} 2. Property 쓰기 테스트")
    test_mac = "AA:BB:CC:DD:EE:FF"
    try:
        obj = bus.get_object(SERVICE, OBJECT_PATH)
        props = dbus.Interface(obj, "org.freedesktop.DBus.Properties")

        #props.Set(INTERFACE, "MacAddress", dbus.String(test_mac))  ##Lighton: this is for qemu
        props.Set(INTERFACE, "MacAddress", dbus.String(test_mac)) 
        time.sleep(0.5)

        mac = str(props.Get(INTERFACE, "MacAddress"))
        ok = mac == test_mac
        record("Property 쓰기", ok, f"설정값={test_mac}, 읽은값={mac}")
    except Exception as e:
        record("Property 쓰기", False, str(e))

# ── 3. PropertiesChanged 시그널 ──────────────────────────────
def test_properties_changed(bus):
    print(f"\n{INFO} 3. PropertiesChanged 시그널 테스트")

    received = {"ok": False, "value": None}
    loop = GLib.MainLoop()

    def on_signal(interface, changed, invalidated):
        if "MacAddress" in changed:
            received["ok"] = True
            received["value"] = str(changed["MacAddress"])
            loop.quit()

    bus.add_signal_receiver(
        on_signal,
        signal_name="PropertiesChanged",
        dbus_interface="org.freedesktop.DBus.Properties",
        bus_name=SERVICE,
        path=OBJECT_PATH,
    )

    # 값 변경으로 시그널 트리거
    def trigger():
        try:
            obj = bus.get_object(SERVICE, OBJECT_PATH)
            props = dbus.Interface(obj, "org.freedesktop.DBus.Properties")
            props.Set(INTERFACE, "MacAddress", dbus.String("11:22:33:44:55:66"))
        except Exception as e:
            print(f"  트리거 오류: {e}")
        return False

    GLib.timeout_add(500, trigger)
    GLib.timeout_add(3000, loop.quit)  # 3초 타임아웃
    loop.run()

    record("PropertiesChanged 시그널", received["ok"],
           f"수신값={received['value']}" if received["ok"] else "시그널 미수신 (3초 타임아웃)")

# ── 4. ObjectMapper 확인 ─────────────────────────────────────
def test_object_mapper(bus):
    print(f"\n{INFO} 4. ObjectMapper 객체 등록 확인")
    try:
        obj = bus.get_object(MAPPER_SVC, MAPPER_PATH)
        mapper = dbus.Interface(obj, MAPPER_IFACE)
        result = mapper.GetObject(OBJECT_PATH, dbus.Array([], signature='s'))

        ok = SERVICE in result
        interfaces = list(result.get(SERVICE, []))
        record("ObjectMapper 등록", ok, f"인터페이스 {len(interfaces)}개 등록됨")
        if ok:
            for iface in interfaces:
                print(f"  - {iface}")
    except Exception as e:
        record("ObjectMapper 등록", False, str(e))

# ── 5. GetAll 테스트 ─────────────────────────────────────────
def test_get_all(bus):
    print(f"\n{INFO} 5. GetAll 테스트")
    try:
        obj = bus.get_object(SERVICE, OBJECT_PATH)
        props = dbus.Interface(obj, "org.freedesktop.DBus.Properties")
        all_props = props.GetAll(INTERFACE)
        ok = "MacAddress" in all_props
        record("GetAll", ok, f"속성 목록: {list(all_props.keys())}")
    except Exception as e:
        record("GetAll", False, str(e))

# ── 6. 서비스 존재 확인 ──────────────────────────────────────
def test_service_exists(bus):
    print(f"\n{INFO} 6. 서비스 존재 확인")
    try:
        services = bus.list_names()
        ok = SERVICE in services
        record("서비스 등록", ok, SERVICE)
    except Exception as e:
        record("서비스 등록", False, str(e))

# ── 결과 요약 ────────────────────────────────────────────────
def print_summary():
    print("\n" + "="*50)
    print("Regression Test 결과 요약")
    print("="*50)
    passed = sum(1 for _, ok, _ in results if ok)
    total = len(results)
    for name, ok, detail in results:
        status = PASS if ok else FAIL
        print(f"  {status} {name}")
    print(f"\n총 {total}개 중 {passed}개 통과", end="  ")
    if passed == total:
        print("\033[92m전체 통과 🎉\033[0m")
    else:
        print(f"\033[91m{total - passed}개 실패\033[0m")

# ── main ─────────────────────────────────────────────────────
def main():
    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SystemBus()

    print("="*50)
    print("OpenBMC Rust D-Bus Regression Test 시작")
    print("="*50)

    test_service_exists(bus)
    test_get_property(bus)
    test_set_property(bus)
    test_properties_changed(bus)
    test_get_all(bus)
    #test_object_mapper(bus)   ##Lighton: ubuntu does not have openbmc object mapper

    print_summary()

if __name__ == "__main__":
    main()
