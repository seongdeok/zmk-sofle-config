# GaggsKey

PC ↔ Sofle(ZMK) 상태/커맨드 링크. 설계: [DESIGN.md](DESIGN.md)

```
gaggskey/
├─ DESIGN.md
├─ build.sh                      # docker 로컬 빌드 (left에 모듈 포함)
├─ firmware/                     # Zephyr/ZMK 모듈
│  ├─ include/gaggskey/proto.h   # 프로토콜 정의 (host와 공유)
│  ├─ src/core.c | transport_usb.c | transport_ble.c
│  ├─ Kconfig, CMakeLists.txt, zephyr/module.yml
│  └─ gaggskey_central.conf
└─ host/                        # cargo workspace (Linux / macOS / Windows)
   ├─ crates/gaggskey-proto/        # proto.h 미러 + 실기기 캡처 테스트 벡터
   ├─ crates/gaggskey-link/         # Transport(USB hidapi, BLE btleplug) + Client
   ├─ apps/gkctl/               # Rust CLI (USB 우선, 없으면 BLE)
   ├─ gkctl/gkctl.py           # 초기 프로토타입 (USB hidraw, stdlib)
   └─ udev/70-gaggskey.rules
```

## 빠른 시작

```sh
./gaggskey/build.sh left          # firmware/nice_sofle_left_oled_gaggskey.uf2
# 왼쪽 반쪽 리셋 2번 → NICENANO 드라이브에 uf2 복사

sudo cp gaggskey/host/udev/70-gaggskey.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger

cd gaggskey/host && cargo build --release
S=target/release/gkctl
$S hello                 # USB든 BLE든 자동
$S watch                 # 레이어 바꾸면 실시간 출력
$S layer 1; $S bt 2; $S out toggle
$S status --json
```

## 데스크탑 앱 (Tauri v2, Linux/macOS/Windows)

```sh
cd gaggskey/host/apps/gaggskey-app
npm install
npx tauri dev                    # 개발 (핫리로드)
npx tauri build                  # 배포 번들 (deb/AppImage, dmg, msi)
npx tauri build --no-bundle      # 실행파일만 (target/release/gaggskey-app)
```

> `cargo build`로 직접 빌드하면 `custom-protocol` 기능이 빠져 dev 서버(localhost:1420)를
> 찾습니다. 실행용 바이너리는 반드시 `tauri build`로 만드세요.

트레이에 상주합니다. 창을 닫으면 트레이로 숨고, 트레이 메뉴의 "종료"로 끝납니다.
