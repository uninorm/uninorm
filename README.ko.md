# uninorm

Unicode NFD 파일명과 텍스트를 NFC로 변환합니다 — macOS, Linux, Windows 지원.

macOS HFS+/APFS는 파일명을 비표준 NFD로 저장해서, 한글·일본어 카나·라틴 악센트 문자가 Linux/Windows에서 깨집니다.

[English](README.md) | 한국어

---

## 설치

**Homebrew (권장):**

```bash
brew tap uninorm/uninorm
brew install uninorm
```

**소스에서 빌드:**

```bash
cargo install --path crates/uninorm-cli
```

---

## 빠른 시작

```bash
# 변경 미리보기 (파일 수정 없음)
uninorm files ~/Downloads --dry-run

# 특정 경로의 NFD 파일명 모두 변환
uninorm files ~/Downloads

# 파일 내용도 함께 변환
uninorm files ~/Downloads --content

# 클립보드 텍스트 변환
uninorm clipboard

# NFC 여부 확인 (비NFC면 exit 1)
uninorm check "東京"

# 텍스트 NFD → NFC 변환 (텍스트 생략 시 stdin에서 읽기)
echo "NFD 텍스트" | uninorm convert
```

---

## `files` — 일회성 변환

```bash
uninorm files <경로> [옵션]
```

| 플래그 | 기본값 | 설명 |
|---|---|---|
| `--dry-run` | false | 미리보기만, 실제 변경 없음 |
| `--no-recursive` | false | 서브디렉토리 재귀 처리 안함 |
| `--content` | false | 파일 내용도 변환 |
| `--follow-symlinks` | false | 심볼릭 링크 추적 |
| `--exclude <PATTERN>` | — | 이름 또는 glob 패턴 일치 항목 제외 (반복 가능) |
| `--max-size <SIZE>` | 100MB | 내용 변환 최대 파일 크기 (예: `50MB`, `1GB`) |
| `-y / --yes` | false | 확인 프롬프트 건너뛰기 |
| `-v / --verbose` | false | 개별 파일 변경 사항 표시 |

---

## `watch` — 감시 항목 관리

백그라운드 데몬의 감시 항목을 관리합니다. 파일이 생성/수정될 때 자동으로 변환됩니다.

```bash
# 감시 경로 추가 (데몬 자동 시작)
uninorm watch add ~/Downloads
uninorm watch add ~/Documents --content --exclude .git --exclude "*.log" --max-size 200MB

# 전체 목록 보기 (번호 포함)
uninorm watch list
#  1. /Users/you/Downloads   [enabled]
#  2. /Users/you/Documents   [disabled]  (content, excludes: .git, *.log)

# 번호로 활성화/비활성화 (쉼표 구분 가능)
uninorm watch enable 1,2
uninorm watch disable 2

# 번호로 삭제
uninorm watch remove 1

# 전체 초기화 및 데몬 중지 (autostart는 유지됨)
uninorm watch reset
```

### 감시 항목 옵션

| 플래그 | 기본값 | 설명 |
|---|---|---|
| `--no-recursive` | false | 서브디렉토리 재귀 처리 안함 |
| `--content` | false | 파일 내용도 변환 |
| `--follow-symlinks` | false | 심볼릭 링크 추적 |
| `--exclude <PATTERN>` | — | 이름 또는 glob 패턴 일치 항목 제외 (반복 가능) |
| `--max-size <SIZE>` | 100MB | 내용 변환 최대 파일 크기 |
| `--debounce <MS>` | 300 | 이벤트 디바운스 간격 (밀리초) |

---

## `daemon` — 백그라운드 데몬 관리

```bash
uninorm daemon start       # 데몬 시작
uninorm daemon stop        # 데몬 중지
uninorm daemon restart     # 데몬 재시작
```

---

## `autostart` — 로그인 시 자동 시작

데몬이 로그인 시 자동으로 시작되도록 등록/해제합니다 (macOS: LaunchAgent, Linux: systemd).

```bash
uninorm autostart on       # 자동 시작 활성화
uninorm autostart off      # 자동 시작 비활성화
```

`uninorm` 명령어를 처음 실행하면 자동으로 autostart가 등록됩니다. `watch reset`은 autostart를 제거하지 않습니다 — 명시적으로 비활성화하려면 `uninorm autostart off`를 사용하세요.

---

## 기타 명령어

```bash
uninorm convert "텍스트"    # 텍스트 NFD → NFC 변환
echo "텍스트" | uninorm convert  # stdin에서 읽기
uninorm convert -c "텍스트" # 변환 후 클립보드에 복사
uninorm clipboard          # 클립보드 텍스트 NFD → NFC 변환
uninorm check "텍스트"      # 텍스트가 NFC인지 확인
uninorm status             # 데몬 상태, autostart, 감시 항목 요약
uninorm log -n 50          # 최근 변환 로그 (마지막 50개)
```

---

## 동작 원리

macOS는 파일명을 쓸 때 `강` (U+AC15)을 낱자(`ᄀ` + `ᅡ` + `ᆼ`)로 분해합니다. 일본어 탁점 카나(`が` → `か` + `゛`), 라틴 악센트(`é` → `e` + `´`)도 마찬가지입니다.

`uninorm`은 이를 다른 시스템이 기대하는 NFC 완성형으로 다시 합칩니다.

> **참고:** macOS의 HFS+ NFD는 Unicode 표준 NFD와 다릅니다. `uninorm`은 [`hfs_nfd`](https://crates.io/crates/hfs_nfd) 크레이트를 사용하여 두 변형을 모두 올바르게 처리합니다. Linux와 Windows에서는 표준 Unicode NFC 정규화를 사용합니다.

---

## 구성

| 크레이트 | 설명 |
|---|---|
| `uninorm-core` | 핵심 라이브러리 — 정규화, 파일 작업, 스캔 |
| `uninorm-cli` | CLI 바이너리 — `files`, `watch`, `daemon`, `autostart`, `convert`, `clipboard`, `check` |
| `uninorm-daemon` | 데몬 라이브러리 — 설정, 컨트롤러, 자동 시작, 백그라운드 감시 |

---

## 라이선스

MIT
