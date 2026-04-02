# uninorm CLI 레퍼런스

[English](cli.md) | 한국어

## 서브커맨드

- [`files`](#files) — 파일/폴더 일괄 변환 (선택적으로 내용 변환 포함)
- [`watch`](#watch) — 백그라운드 데몬의 감시 항목 관리
- [`daemon`](#daemon) — 백그라운드 데몬 관리 (start/stop/restart)
- [`autostart`](#autostart) — 로그인 시 데몬 자동 시작 등록/해제 (on/off)
- [`convert`](#convert) — 텍스트 NFD → NFC 변환
- [`clipboard`](#clipboard) — 클립보드 텍스트 변환
- [`check`](#check) — 텍스트 NFC 여부 확인
- [`log`](#log) — 최근 변환 로그 보기
- [`status`](#status) — 데몬 상태, autostart, 감시 항목 요약

---

## `files`

디렉토리(또는 단일 파일)를 재귀적으로 스캔하여 NFD 파일명을 NFC로 변환합니다. 파일 내용도 함께 변환할 수 있습니다.

```
uninorm files <경로> [옵션]
```

**인수**

| 인수 | 설명 |
|---|---|
| `경로` | 처리할 파일 또는 디렉토리 (필수) |

**옵션**

| 플래그 | 기본값 | 설명 |
|---|---|---|
| `--dry-run` | false | 실제 변경 없이 미리보기만 |
| `--no-recursive` | false | 서브디렉토리 재귀 처리 안함 |
| `--content` | false | 파일 내용도 변환 |
| `--follow-symlinks` | false | 심볼릭 링크 추적 |
| `--exclude <PATTERN>` | — | 이름 또는 glob 패턴 일치 항목 제외 (반복 가능) |
| `--max-size <SIZE>` | 100MB | 내용 변환 최대 파일 크기 (예: `50MB`, `1GB`) |
| `--no-global-ignore` | false | 글로벌 ignore 패턴 적용 안함 |
| `-y / --yes` | false | 확인 프롬프트 건너뛰기 |
| `-v / --verbose` | false | 개별 파일 변경 사항 표시 |
| `--json` | false | 결과를 JSON으로 출력 (스크립팅/CI용) |

**예시**

```bash
# 변경 미리보기
uninorm files ~/Downloads --dry-run

# ~/Downloads 아래 모든 NFD 파일명 변환
uninorm files ~/Downloads

# 파일 내용도 함께 변환 (소스 코드 내 NFD 문자열 등)
uninorm files ~/Downloads --content

# .git, node_modules 제외
uninorm files ~/project --exclude .git --exclude node_modules

# 단일 파일
uninorm files ~/Downloads/한글파일.txt
```

**출력**

```
Scanned:  1024
Renamed:  17
Content:  3
```

변환 중 오류가 발생하면 종료 코드 `1`을 반환합니다.

**참고**

- 내용 변환은 임시 파일에 먼저 쓴 뒤 원본으로 이름 변경하는 방식으로 원자적으로 수행됩니다.
- `--exclude`는 항목의 이름(전체 경로가 아님)에 대해서만 적용됩니다.

---

## `watch`

백그라운드 데몬의 감시 항목을 관리합니다. 파일이 생성/수정될 때 자동으로 변환됩니다.

```
uninorm watch <서브커맨드>
```

### `watch add`

감시 항목을 추가하거나 업데이트합니다. 데몬이 실행 중이 아니면 자동으로 시작합니다.

```bash
uninorm watch add <경로> [옵션]
```

| 플래그 | 기본값 | 설명 |
|---|---|---|
| `--no-recursive` | false | 서브디렉토리 재귀 처리 안함 |
| `--content` | false | 파일 내용도 변환 |
| `--follow-symlinks` | false | 심볼릭 링크 추적 |
| `--exclude <PATTERN>` | — | 이름 또는 glob 패턴 일치 항목 제외 (반복 가능) |
| `--max-size <SIZE>` | 100MB | 내용 변환 최대 파일 크기 |
| `--debounce <MS>` | 300 | 이벤트 디바운스 간격 (밀리초) |

### `watch list`

전체 감시 항목을 번호와 함께 표시합니다.

```bash
uninorm watch list
#  1. /Users/you/Downloads   [enabled]
#  2. /Users/you/Documents   [disabled]  (content, excludes: .git, *.log)
```

### `watch enable` / `watch disable`

번호로 활성화/비활성화합니다 (쉼표 구분 가능).

```bash
uninorm watch enable 1,2
uninorm watch disable 2
```

### `watch remove`

번호로 항목을 삭제합니다 (쉼표 구분 가능).

```bash
uninorm watch remove 1
```

### `watch reset`

전체 감시 항목을 삭제하고 데몬을 중지합니다. autostart는 유지됩니다.

```bash
uninorm watch reset
uninorm watch reset -y   # 확인 프롬프트 건너뛰기
```

---

## `daemon`

백그라운드 데몬 프로세스를 관리합니다. `systemctl start/stop`과 유사합니다.

```bash
uninorm daemon start       # 데몬 시작
uninorm daemon stop        # 데몬 중지
uninorm daemon restart     # 데몬 재시작
```

데몬은 `uninorm watch add`로 설정한 경로를 감시하고, 파일 시스템 이벤트 발생 시 NFD 파일명(및 선택적으로 내용)을 자동으로 변환합니다.

---

## `autostart`

로그인 시 데몬이 자동으로 시작되도록 등록/해제합니다. `systemctl enable/disable`과 유사합니다.

- **macOS:** LaunchAgent plist 설치
- **Linux:** systemd user 서비스 설치

```bash
uninorm autostart on       # 자동 시작 활성화
uninorm autostart off      # 자동 시작 비활성화
```

`uninorm` 명령어를 처음 실행하면 자동으로 autostart가 등록됩니다. `watch reset`은 autostart를 제거하지 않습니다 — 명시적으로 비활성화하려면 `uninorm autostart off`를 사용하세요.

---

## `convert`

텍스트를 NFD에서 NFC로 변환하여 출력합니다. 텍스트를 생략하면 stdin에서 읽습니다.

```
uninorm convert [텍스트] [옵션]
```

| 플래그 | 설명 |
|---|---|
| `-c / --clipboard` | 변환 결과를 클립보드에 복사 |
| `--json` | 결과를 JSON으로 출력 |

**예시**

```bash
uninorm convert "NFD 텍스트"
echo "NFD 텍스트" | uninorm convert
uninorm convert -c "텍스트"   # 변환 후 클립보드에 복사
uninorm convert --json "NFD 텍스트"   # {"input":"...","output":"...","changed":true}
```

---

## `clipboard`

클립보드의 텍스트를 읽어 NFD를 NFC로 변환한 뒤 다시 클립보드에 씁니다.

```
uninorm clipboard
```

**예시**

```bash
uninorm clipboard
# → "Clipboard converted to NFC."
# → "Clipboard is already NFC — no changes made."
```

---

## `check`

문자열이 이미 NFC로 정규화되어 있는지 확인합니다. NFC가 아니면 종료 코드 `1`을 반환합니다.

```
uninorm check <텍스트> [옵션]
```

| 플래그 | 설명 |
|---|---|
| `--json` | 결과를 JSON으로 출력 |

**예시**

```bash
uninorm check "東京"
# ✓ Already NFC

uninorm check $'か\u3099'   # か + 결합 탁점 (NFD)
# ✗ NOT NFC — converted form: が

# 스크립트에서 활용
if ! uninorm check "$filename"; then
  echo "파일명 정규화 필요"
fi
```

---

## `log`

최근 변환 로그 항목을 표시합니다.

```
uninorm log [-n N]
```

| 플래그 | 기본값 | 설명 |
|---|---|---|
| `-n / --lines N` | 50 | 표시할 최근 줄 수 |

**로그 위치:** `~/.config/uninorm/uninorm.log`

---

## `status`

데몬 상태, autostart 상태, 감시 항목 요약, 최근 로그를 표시합니다.

```
uninorm status
```

**출력 예시**

```
Daemon running (PID 12345)
Autostart: on
Watch entries: 2/3 enabled
Use `uninorm watch list` for details.

Recent activity:
  [2024-03-09 14:23:15] Renamed: 한글파일.txt → 한글파일.txt
  [2024-03-09 14:30:02] Renamed: café.txt → café.txt
```

---

## 글로벌 ignore

`~/.config/uninorm/ignore` 파일을 생성하면 항상 제외할 패턴을 정의할 수 있습니다. `watch` 데몬과 `files` 명령 모두에 기본 적용됩니다.

```
# ~/.config/uninorm/ignore
.git
node_modules
target
__pycache__
.DS_Store
*.pyc
```

형식: 한 줄에 glob 패턴 하나, `#`은 주석, 빈 줄 무시.

`files` 명령은 `--no-global-ignore`로 비활성화할 수 있습니다. 데몬은 항상 글로벌 ignore를 적용하며, 항목별 제외는 `--exclude`를 사용하세요.

---

## 종료 코드

| 코드 | 의미 |
|---|---|
| `0` | 성공 (`check`의 경우 이미 NFC) |
| `1` | `files` 실행 중 오류 발생; `check`의 경우 NFC가 아님 |
