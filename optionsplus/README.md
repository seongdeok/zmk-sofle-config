# Sofle Options+

PC ↔ Sofle(ZMK) 상태/커맨드 링크. 설계: [DESIGN.md](DESIGN.md)

```
optionsplus/
├─ DESIGN.md
├─ build.sh                      # docker 로컬 빌드 (left에 모듈 포함)
├─ firmware/                     # Zephyr/ZMK 모듈
│  ├─ include/optionsplus/proto.h   # 프로토콜 정의 (host와 공유)
│  ├─ src/core.c | transport_usb.c | transport_ble.c
│  ├─ Kconfig, CMakeLists.txt, zephyr/module.yml
│  └─ optionsplus_central.conf
└─ host/                        # cargo workspace (Linux / macOS / Windows)
   ├─ crates/sofle-proto/        # proto.h 미러 + 실기기 캡처 테스트 벡터
   ├─ crates/sofle-link/         # Transport(USB hidapi, BLE btleplug) + Client
   ├─ apps/sofctl/               # Rust CLI (USB 우선, 없으면 BLE)
   ├─ sofctl/sofctl.py           # 초기 프로토타입 (USB hidraw, stdlib)
   └─ udev/70-sofle-optionsplus.rules
```

## 빠른 시작

```sh
./optionsplus/build.sh left          # firmware/nice_sofle_left_oled_optionsplus.uf2
# 왼쪽 반쪽 리셋 2번 → NICENANO 드라이브에 uf2 복사

sudo cp optionsplus/host/udev/70-sofle-optionsplus.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger

cd optionsplus/host && cargo build --release
S=target/release/sofctl
$S hello                 # USB든 BLE든 자동
$S watch                 # 레이어 바꾸면 실시간 출력
$S layer 1; $S bt 2; $S out toggle
$S status --json
```
