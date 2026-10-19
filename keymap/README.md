# 공통 키맵 (Corne base + 키보드별 오버레이)

세 키보드(Corne · Sofle · Kyria)는 같은 7레이어(BAS NAV SYM NUM MSE FUN MED)와
같은 behavior를 쓴다. 키맵은 **한 곳만 고친다.**

```
config/corne.keymap        ← ★ 베이스. 평소처럼 손으로 고친다 (Corne 3x6+3, 42키)
keymap/sofle.toml          ← Sofle이 Corne과 "다른 것"만
keymap/kyria.toml          ← Kyria가 Corne과 "다른 것"만
keymap/gen.py              → config/sofle.keymap, config/gaggs_kyria.keymap 생성 (직접 고치지 말 것)
keymap/drafts/             ← 검토 중인 키맵 초안 (빌드에 안 쓰임)
```

## 매일 쓰는 흐름

1. `config/corne.keymap` 수정
2. `python3 keymap/gen.py` (또는 `./build.sh` — 빌드 전에 자동 실행)
3. 세 키보드 모두 반영됨. 오버레이로 덮어쓴 키만 각자 값 유지

생성 파일이 낡았으면 GitHub Actions의 `keymap-check`가 실패한다(`gen.py --check`).

## 오버레이(toml) 문법

```toml
output = "config/sofle.keymap"
includes = ["dt-bindings/zmk/ext_power.h"]     # 베이스 include에 추가

# 물리 키 순서(한 줄 = 한 행). 숫자 = Corne 키 번호, 이름 = Corne에 없는 키
# 이름의 끝 숫자를 뗀 것이 그룹(num0..num11 → num)
layout = '''
num0 num1 ... num11
0 1 2 ... 11
...
thumb0 thumb1 36 37 38 39 40 41 thumb2 thumb3
'''

[defines]              # hold-trigger용 키 위치 목록 (물리 번호)
LEFT_KEYS = "0 1 2 ..."

[sensors]              # 엔코더 (레이어별, 없으면 생략)
BAS = "&inc_dec_kp C_VOL_DN C_VOL_UP &inc_dec_kp PG_DN PG_UP"

[behaviors.hmr]        # 베이스 behavior 속성 덮어쓰기, false = 속성 삭제
flavor = '"tap-preferred"'
[behaviors.mo_sk]      # 베이스에 없는 behavior는 새로 생성
_node = "momentary_sticky_layer"

[override.NAV]         # Corne 키 번호 = 이 키보드에서 쓸 바인딩
13 = "&kp LCTRL"       # corne: &kp LEFT

[extras.BAS]           # Corne에 없는 키. 안 적은 칸은 &trans
num = "&kp GRAVE  &kp N1 ..."
```

Corne 키 번호:

```
 0  1  2  3  4  5     6  7  8  9 10 11
12 13 14 15 16 17    18 19 20 21 22 23
24 25 26 27 28 29    30 31 32 33 34 35
         36 37 38    39 40 41
```

베이스에 콤보를 추가하면 `key-positions`는 Corne 번호로 쓰면 되고, 각 키보드에 맞게 자동 변환된다.

## 키맵 통일하기

지금 오버레이는 통합 전 각 키보드 키맵을 **그대로 재현**하도록 만들어져 있다
(`gen.py --compare sofle <원본>` 로 cpp 해석 결과까지 동일함을 확인).
`[override.*]` 줄을 지울수록 Corne과 같아진다 — 각 줄 주석에 Corne 값이 적혀 있다.

## 주의

- ZMK Studio에서 저장한 키맵은 파일보다 우선한다. 새 펌웨어 키맵을 보려면
  Studio에서 **Restore Stock Settings**.
- 레이어 번호/이름은 세 키보드가 같아야 한다 (GaggsKey 키맵 뷰, `lt_th N`).
