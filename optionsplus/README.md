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
└─ host/
   ├─ sofctl/sofctl.py           # USB hidraw CLI (stdlib only)
   └─ udev/70-sofle-optionsplus.rules
```

## 빠른 시작

```sh
./optionsplus/build.sh left          # firmware/nice_sofle_left_oled_optionsplus.uf2
# 왼쪽 반쪽 리셋 2번 → NICENANO 드라이브에 uf2 복사

sudo cp optionsplus/host/udev/70-sofle-optionsplus.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger

./optionsplus/host/sofctl/sofctl.py hello
./optionsplus/host/sofctl/sofctl.py watch      # 레이어 바꾸면 실시간 출력
./optionsplus/host/sofctl/sofctl.py layer 1
./optionsplus/host/sofctl/sofctl.py bt 2
./optionsplus/host/sofctl/sofctl.py status --json
```
