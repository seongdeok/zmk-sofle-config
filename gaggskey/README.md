# GaggsKey

ZMK 키보드(Sofle)를 위한 Logitech Options+ 같은 호스트 연동 시스템.
PC에서 키보드 상태를 실시간으로 보고, 명령을 보내고, 레이어별 키맵을 확인합니다.

- **상태 표시** — 현재 레이어, 좌/우 배터리, 출력(USB/BLE), BT 프로필 연결 상태
- **원격 명령** — 레이어 전환/토글, BT 프로필 선택·페어링 해제, 출력 전환, 재부팅/부트로더
- **키맵 뷰어** — 펌웨어에서 키맵과 물리 배치를 직접 읽어와 레이어별 키 값을 키보드 그림으로 표시
- **트레이 앱** (Tauri v2, Linux / macOS / Windows) + **CLI** `gkctl`
- **전송** — USB(벤더 HID) 우선, 없으면 BLE(커스텀 GATT). 별도 드라이버/데몬 없음

설계 상세: [DESIGN.md](DESIGN.md)

## 구조

```
gaggskey/
├─ DESIGN.md
├─ build.sh                        # docker 로컬 펌웨어 빌드 (왼쪽=central에 모듈 포함)
├─ firmware/                       # Zephyr/ZMK 모듈
│  ├─ include/gaggskey/proto.h     # 와이어 프로토콜 (host와 공유되는 단일 정의)
│  ├─ src/core.c                   # 상태 캐시, 변경 push, 커맨드 처리, 키맵 조회
│  ├─ src/transport_usb.c          # 두 번째 HID 인터페이스 (usage page 0xFF0B)
│  ├─ src/transport_ble.c          # GATT 서비스 5f0b0001-6761-6767-736b-657900000000
│  ├─ Kconfig, CMakeLists.txt, zephyr/module.yml
│  └─ gaggskey_central.conf
└─ host/                           # cargo workspace
   ├─ crates/gaggskey-proto/       # proto.h 미러, 키 라벨 변환, 실기기 캡처 테스트 벡터
   ├─ crates/gaggskey-link/        # Transport(USB hidapi / BLE btleplug) + Client
   ├─ crates/gaggskey-core/        # 연결 유지·재연결 워커 (앱에서 사용)
   ├─ apps/gkctl/                  # CLI
   ├─ apps/gaggskey-app/           # Tauri v2 + Svelte 5 트레이 앱
   ├─ gkctl/gkctl.py               # 초기 프로토타입 (USB hidraw, stdlib)
   ├─ install-linux.sh
   ├─ install-macos.sh
   └─ udev/70-gaggskey.rules
```

## 1. 펌웨어

```sh
./gaggskey/build.sh left           # → firmware/nice_sofle_left_oled_gaggskey.uf2
```

왼쪽 반쪽(central)만 다시 플래시하면 됩니다 (리셋 2번 → `NICENANO` 드라이브에 uf2 복사).
오른쪽(peripheral)은 기존 펌웨어 그대로 둡니다.

다른 ZMK 키보드에 넣을 때는 `ZMK_EXTRA_MODULES`에 `gaggskey/firmware`를 추가하고
`gaggskey_central.conf` 내용을 central 쪽 conf에 합치면 됩니다.

| Kconfig | 기본 | 설명 |
|---|---|---|
| `CONFIG_ZMK_GAGGSKEY` | n | 모듈 활성화 (central에서만) |
| `CONFIG_ZMK_GAGGSKEY_USB` / `_BLE` | y | 전송 경로별 on/off |
| `CONFIG_ZMK_GAGGSKEY_DANGEROUS_CMDS` | y | 재부팅/부트로더 명령 허용 |
| `CONFIG_ZMK_GAGGSKEY_PUSH_DEBOUNCE_MS` | 30 | 상태 push 묶음 간격 |
| `CONFIG_USB_HID_DEVICE_COUNT=2`, `CONFIG_HID_INTERRUPT_EP_MPS=32` | — | USB 전송에 필요 |

BLE 특성은 암호화된 연결(이미 페어링된 호스트)에서만 열립니다. 재페어링은 필요 없습니다.

## 2. 앱 설치

### Linux

```sh
gaggskey/host/install-linux.sh                 # 빌드 + 설치 + 로그인 시 자동 실행 + udev 룰
gaggskey/host/install-linux.sh --no-build      # 이미 빌드된 바이너리만 설치
gaggskey/host/install-linux.sh --no-autostart
gaggskey/host/install-linux.sh --uninstall
```

필요: Rust, Node.js 18+, `webkit2gtk-4.1`, `libayatana-appindicator`(트레이), BlueZ.

설치 위치:
`~/.local/bin/{gaggskey-app,gkctl}`, `~/.local/share/applications/gaggskey.desktop`,
`~/.config/autostart/gaggskey.desktop`, `/etc/udev/rules.d/70-gaggskey.rules`(USB 접근 권한, pkexec/sudo로 설치).
코드를 고친 뒤에는 스크립트를 다시 실행하면 빌드·교체·재시작까지 한 번에 됩니다.

### macOS

Mac에서 직접 빌드합니다 (Tauri는 macOS 크로스 빌드를 지원하지 않음).

```sh
# 사전 준비 (한 번)
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
brew install node

git clone -b feature/gaggskey https://github.com/seongdeok/zmk-sofle-config.git
cd zmk-sofle-config/gaggskey/host
./install-macos.sh                  # 빌드 + /Applications 설치 + 로그인 항목
./install-macos.sh --user           # ~/Applications 에 설치 (sudo 불필요)
./install-macos.sh --no-build       # 이미 빌드된 GaggsKey.app 만 설치
./install-macos.sh --no-autostart
./install-macos.sh --uninstall
```

- 로컬 빌드라 서명이 없으므로 스크립트가 quarantine 속성을 지우고 ad-hoc 서명합니다.
- 처음 실행 시 **블루투스 권한** 요청을 허용하세요
  (거부했다면 시스템 설정 → 개인정보 보호 및 보안 → 블루투스에서 GaggsKey 켜기).
- 로그인 자동 실행은 `~/Library/LaunchAgents/dev.gaggs.gaggskey.plist`로 등록됩니다.
- `gkctl`은 `~/.local/bin`에 설치됩니다.
- macOS는 USB HID 접근에 별도 권한이 필요 없습니다.

### Windows

```powershell
cd gaggskey\host\apps\gaggskey-app
npm install
npx tauri build                      # → target\release\bundle\msi\*.msi
```

### 개발

```sh
cd gaggskey/host/apps/gaggskey-app
npm install
npx tauri dev                        # 핫리로드
npx tauri build --no-bundle          # 실행파일만 (target/release/gaggskey-app)
```

> `cargo build`로 앱을 빌드하면 `custom-protocol` 기능이 빠져서 dev 서버(localhost:1420)를 찾다가
> "connection refused"가 납니다. 실행용 바이너리는 반드시 `tauri build`로 만드세요.

## 3. 사용법

### 앱

트레이에 상주합니다. 창을 닫으면 트레이로 숨고, 트레이 메뉴 "종료"로 끝납니다.

- **레이어** — 클릭: 전환, 우클릭: 토글
- **키맵** — "자동"은 키보드의 현재 레이어를 따라가고, 레이어 탭을 누르면 그 레이어를 고정해서 미리봅니다.
  `&trans` 키는 아래 레이어에서 실제로 입력되는 값을 흐리게 보여줍니다. 키에 마우스를 올리면 원본 바인딩이 나옵니다.
  테두리 색: 레이어=주황, 모디파이어=파랑, BT/출력=보라, 마우스=초록, 미디어=분홍, 시스템=빨강.
- **고급** — BT 페어링 해제, 재부팅, 부트로더 진입 (확인 창 있음)

### gkctl

```sh
gkctl hello                 # 키보드 정보 (USB/BLE 자동 선택)
gkctl status [--json]       # 현재 상태 한 번
gkctl watch                 # 상태 변화 스트리밍
gkctl layers                # 레이어 목록
gkctl keymap [LAYER]        # 레이어별 키 값 덤프 (--json 가능)
gkctl layer 1 | toggle 2 | on 3 | off 3
gkctl bt 2 | bt next | bt prev | bt-clear
gkctl out usb | ble | toggle
gkctl reset | bootloader
```

## 4. 프로토콜 요약

32바이트 고정 프레임 `[0x5F][ver=1][type][seq][len][payload ≤27]`, little-endian.
USB는 SET_REPORT(호스트→키보드) / interrupt IN(키보드→호스트), BLE는 특성 하나로 write + notify.

| type | 방향 | 내용 |
|---|---|---|
| `0x01` HELLO | ↔ | 이름, 기능 비트, 레이어 수, BT 프로필 수 |
| `0x02` GET_STATE / `0x03` STATE | ↔ | 상태 (요청 응답 + 변경 시 push, seq=0) |
| `0x10` CMD / `0x11` ACK | ↔ | `[cmd][args]` → `[cmd][status][data]` |
| `0x7F` ERROR | ← | 잘못된 프레임 |

| cmd | 인자 → 응답 |
|---|---|
| `0x01–0x04` | 레이어 to / on / off / toggle `[id]` |
| `0x05` | 레이어 이름 `[id]` → 이름 |
| `0x10–0x13` | BT 선택 `[n]` / 다음 / 이전 / 현재 프로필 해제 |
| `0x20–0x21` | 출력 선택 `[0=USB,1=BLE]` / 토글 |
| `0x30` KEYMAP_INFO | → `[키 수][레이어 수][behavior 수][배치 키 수]` |
| `0x31` BEHAVIOR_NAME | `[idx]` → behavior 이름 |
| `0x32` KEYMAP_GET | `[layer][start]` → `[n]` + n × (`[bidx][LEB128 p1][LEB128 p2]`) |
| `0x33` LAYOUT_GET | `[pos]` → int16 × 7 (w, h, x, y, r, rx, ry; 1/100 키·도 단위) |
| `0x7E` / `0x7F` | 재부팅 / 부트로더 (가드 바이트 `B0 07`) |

키 라벨 변환(HID usage → `A`, `Ctrl+C`, `Vol+` …)은 호스트의 `gaggskey-proto::keymap`이 담당합니다.
커스텀 hold-tap은 "p1 < 32 → 레이어, 아니면 모디파이어/키"로 추정해 `탭 / 홀드` 두 줄로 표시합니다.

## 상태 / 알려진 제한

| 항목 | 상태 |
|---|---|
| Linux USB / BLE | 실기기 검증 완료 |
| 키맵 조회 (USB, 7레이어×60키) | 약 0.16초 |
| macOS | 설치 스크립트 준비, 실기기 미검증. BLE는 이미 연결된 키보드 탐색이 안 될 수 있음 → USB 연결 권장 |
| Windows | 미검증 |
| ZMK Studio 등으로 키맵 변경 | 자동 감지 안 됨 — 앱 재연결 시 다시 읽음 |
