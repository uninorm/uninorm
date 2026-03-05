# mac-uninorm

macOS의 NFD 파일명과 텍스트를 NFC로 변환합니다.

macOS HFS+/APFS는 파일명을 비표준 NFD로 저장해서, 한글·일본어 카나·라틴 악센트 문자가 Linux/Windows에서 깨집니다.

> English: [README.md](README.md)

---

## CLI

### 설치

**Homebrew (권장):**

```bash
brew tap sts07142/uninorm
brew install uninorm
```

**소스에서 빌드:**

```bash
cargo install --path crates/uninorm-cli
```

### 사용법

```bash
# 변경 미리보기 (파일 수정 없음)
uninorm files ~/Downloads --dry-run

# 파일/폴더명 재귀 변환
uninorm files ~/Downloads

# 파일 내용도 함께 변환
uninorm files ~/Downloads --content

# 클립보드 텍스트 변환
uninorm clipboard

# NFC 여부 확인 (비NFC면 exit 1)
uninorm check "東京"
```

### `files` 옵션

| 플래그 | 기본값 | 설명 |
|---|---|---|
| `--dry-run` | false | 미리보기만, 실제 변경 없음 |
| `-r / --recursive` | true | 서브디렉토리 재귀 처리 |
| `--content` | false | 파일 내용도 변환 |
| `--follow-symlinks` | false | 심볼릭 링크 추적 |

---

## GUI (macOS 메뉴바 앱)

폴더를 감시하여 파일이 생성되거나 이름이 바뀔 때 NFD 파일명을 자동으로 NFC로 변환하는 macOS 메뉴바 앱입니다.

### 설치

**소스에서 빌드 (Rust + macOS 필요):**

```bash
# 직접 실행

# 배포 가능한 .app 번들 빌드 (cargo-bundle 필요)
cargo install cargo-bundle
make bundle
# → target/release/bundle/osx/uninorm.app
```

### 기능

| 기능 | 설명 |
|---|---|
| **메뉴바** | 메뉴바 아이콘으로 실행 (Dock에서 숨김) |
| **파일 브라우저** | 계층 / 목록 / 아이콘 / 갤러리 뷰로 파일 탐색 |
| **감시 경로** | NFD 파일명 감시할 폴더 등록 |
| **자동 변환** | 파일 생성/이름 변경 이벤트 발생 시 자동으로 파일명 변환 |
| **전체 검색** | 기존 NFD 파일명을 수동으로 즉시 검색·변환 |
| **즐겨찾기** | 자주 쓰는 경로 저장 및 원클릭으로 감시 경로 추가 |
| **활동 로그** | 앱 내 로그 + `~/.config/uninorm/uninorm.log` 파일 저장 |
| **로그인 자동 실행** | 로그인 시 자동 시작 LaunchAgent 설정 |
| **언어** | 영어 / 한국어 UI (설정 저장) |
| **드래그 앤 드롭** | 폴더를 창에 드래그하여 감시 경로로 추가 |

### 설정 파일

설정은 `~/.config/uninorm/config.json`에 저장됩니다:

```json
{
  "watched_paths": ["/Users/you/Downloads"],
  "inactive_paths": [],
  "bookmarks": [],
  "lang": "Korean"
}
```

### 자동 변환이 항상 실행되지 않는 이유?

APFS는 파일 저장 시 파일명을 NFC로 정규화하므로, 현대 Mac에서 새로 만든 파일은 이미 NFC입니다. 자동 변환은 외부 소스(USB 드라이브, 네트워크 공유, 구형 HFS+ 볼륨)에서 NFD 파일명이 들어올 때 실행됩니다. 기존 NFD 파일을 변환하려면 **전체 검색** 버튼을 사용하세요.

---

## 동작 원리

macOS는 파일명을 쓸 때 `강` (U+AC15)을 낱자(`ᄀ` + `ᅡ` + `ᆼ`)로 분해합니다. 일본어 탁점 카나(`が` → `か` + `゛`), 라틴 악센트(`é` → `e` + `´`)도 마찬가지입니다.

`uninorm`은 이를 다른 시스템이 기대하는 NFC 완성형으로 다시 합칩니다.

> **참고:** macOS의 HFS+ NFD는 Unicode 표준 NFD와 다릅니다. `uninorm`은 두 변형을 모두 올바르게 처리합니다.

---

## 구성

| 크레이트 | 설명 | 상태 |
|---|---|---|
| `uninorm-core` | 핵심 라이브러리 (크로스플랫폼) | 완료 |
| `uninorm-cli` | CLI 바이너리 | 완료 |

---

## 라이선스

MIT
