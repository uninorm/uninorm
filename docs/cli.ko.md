# uninorm CLI 레퍼런스

> English: [cli.md](cli.md)

## 서브커맨드

- [`files`](#files) — 파일/폴더 일괄 변환 (선택적으로 내용 변환 포함)
- [`watch`](#watch) — 실시간 감시: 파일 생성/이름 변경 시 자동 변환
- [`log`](#log) — 최근 변환 로그 보기
- [`clipboard`](#clipboard) — 클립보드 텍스트 변환
- [`check`](#check) — 텍스트 NFC 여부 확인

---

## `files`

디렉토리(또는 단일 파일)를 재귀적으로 스캔하여 NFD 파일명을 NFC로 변환합니다. 파일 내용도 함께 변환할 수 있습니다.

```
uninorm files [PATH] [OPTIONS]
```

**인수**

| 인수 | 기본값 | 설명 |
|---|---|---|
| `PATH` | `.` (현재 디렉토리) | 처리할 파일 또는 디렉토리 |

**옵션**

| 플래그 | 기본값 | 설명 |
|---|---|---|
| `--dry-run` | false | 실제 변경 없이 미리보기만 |
| `-r / --recursive` | true | 서브디렉토리 재귀 처리 |
| `--content` | false | 파일 내용도 변환 |
| `--follow-symlinks` | false | 심볼릭 링크 추적 |
| `--exclude <PATTERN>` | — | 이름이 일치하는 항목 제외 (반복 가능) |

**예시**

```bash
# 현재 디렉토리에서 변경될 내용 미리보기
uninorm files --dry-run

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

- 100 MB를 초과하는 파일은 내용 변환에서 제외됩니다.
- 내용 변환은 임시 파일에 먼저 쓴 뒤 원본으로 이름 변경하는 방식으로 원자적으로 수행됩니다.
- `--exclude`는 항목의 이름(전체 경로가 아님)에 대해서만 적용됩니다.

---

## `watch`

디렉토리를 감시하다가 파일이 생성되거나 이름이 바뀔 때 NFD 파일명을 자동으로 NFC로 변환합니다. macOS에서는 FSEvents, Linux에서는 inotify, Windows에서는 ReadDirectoryChanges를 사용합니다.

```
uninorm watch [PATH...] [OPTIONS]
```

**인수**

| 인수 | 기본값 | 설명 |
|---|---|---|
| `PATH` | `.` (현재 디렉토리) | 감시할 디렉토리 (공백으로 여러 개 지정 가능) |

**옵션**

| 플래그 | 기본값 | 설명 |
|---|---|---|
| `--exclude <PATTERN>` | — | 이름이 일치하는 항목 제외 (반복 가능) |

**예시**

```bash
# 현재 디렉토리 감시
uninorm watch

# 여러 경로 감시
uninorm watch ~/Downloads ~/Desktop

# .git 제외하고 감시
uninorm watch ~/project --exclude .git

# 백그라운드 실행 (셸 작업 제어)
uninorm watch ~/Downloads &
```

**출력**

```
Watching: /Users/you/Downloads
Press Ctrl+C to stop.

Renamed: 한글파일.txt → 한글파일.txt
```

변환 내역은 stdout에 출력되며 `~/.config/uninorm/uninorm.log` 파일에도 기록됩니다.

**Ctrl+C**를 누르면 정상적으로 종료됩니다.

**참고**

- 현대 macOS의 APFS는 새로 생성된 파일명을 자동으로 NFC로 정규화합니다. 따라서 `watch`는 주로 외부 소스(USB 드라이브, 네트워크 공유, 구형 HFS+ 볼륨)에서 복사된 파일에 반응합니다.
- `watch`는 시작 시 전체 스캔을 하지 않습니다. 기존 NFD 파일 변환은 먼저 `uninorm files`를 사용하세요.

---

## `log`

`watch`가 기록한 변환 로그의 최근 항목을 표시합니다.

```
uninorm log [-n N]
```

**옵션**

| 플래그 | 기본값 | 설명 |
|---|---|---|
| `-n / --lines N` | 50 | 표시할 최근 줄 수 |

**로그 위치:** `~/.config/uninorm/uninorm.log`

**예시**

```bash
# 마지막 50개 항목 (기본값)
uninorm log

# 마지막 100개 항목
uninorm log -n 100

# 전체 항목 (페이저 사용)
uninorm log -n 99999 | less
```

**출력 예시**

```
[2024-03-09 14:22:01] Watching: /Users/you/Downloads
[2024-03-09 14:23:15] Renamed: 한글파일.txt → 한글파일.txt
[2024-03-09 14:30:02] Watch stopped.

(3 total entries, showing last 3)
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

붙여넣기 후 처리 또는 단축키에 연결해두면 편리합니다.

---

## `check`

문자열이 이미 NFC로 정규화되어 있는지 확인합니다. NFC가 아니면 종료 코드 `1`을 반환합니다.

```
uninorm check TEXT
```

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

## 로그 파일

`watch`는 아래 경로에 타임스탬프와 함께 항목을 기록합니다:

```
~/.config/uninorm/uninorm.log
```

디렉토리는 첫 실행 시 자동으로 생성됩니다.

---

## 종료 코드

| 코드 | 의미 |
|---|---|
| `0` | 성공 (`check`의 경우 이미 NFC) |
| `1` | `files` 실행 중 오류 발생; `check`의 경우 NFC가 아님 |
