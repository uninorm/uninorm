# uninorm

Unicode NFD 파일명과 텍스트를 NFC로 변환합니다 — macOS, Linux, Windows 지원.

macOS HFS+/APFS는 파일명을 비표준 NFD로 저장해서, 한글·일본어 카나·라틴 악센트 문자가 Linux/Windows에서 깨집니다.

> English: [README.md](README.md)

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
# 현재 디렉토리 변경 미리보기 (파일 수정 없음)
uninorm files --dry-run

# 특정 경로의 NFD 파일명 모두 변환
uninorm files ~/Downloads

# 디렉토리 감시 — 파일이 생성/이름변경 될 때 자동 변환
uninorm watch ~/Downloads

# 최근 변환 로그 보기
uninorm log

# 클립보드 텍스트 변환
uninorm clipboard

# NFC 여부 확인 (비NFC면 exit 1)
uninorm check "東京"
```

전체 레퍼런스: [docs/cli.ko.md](docs/cli.ko.md)

---

## `files` 옵션

| 플래그 | 기본값 | 설명 |
|---|---|---|
| `--dry-run` | false | 미리보기만, 실제 변경 없음 |
| `-r / --recursive` | true | 서브디렉토리 재귀 처리 |
| `--content` | false | 파일 내용도 변환 |
| `--follow-symlinks` | false | 심볼릭 링크 추적 |
| `--exclude <PATTERN>` | — | 이름이 일치하는 항목 제외 (반복 가능) |

---

## 동작 원리

macOS는 파일명을 쓸 때 `강` (U+AC15)을 낱자(`ᄀ` + `ᅡ` + `ᆼ`)로 분해합니다. 일본어 탁점 카나(`が` → `か` + `゛`), 라틴 악센트(`é` → `e` + `´`)도 마찬가지입니다.

`uninorm`은 이를 다른 시스템이 기대하는 NFC 완성형으로 다시 합칩니다.

> **참고:** macOS의 HFS+ NFD는 Unicode 표준 NFD와 다릅니다. `uninorm`은 두 변형을 모두 올바르게 처리합니다. Linux와 Windows에서는 표준 Unicode NFC 정규화를 사용합니다.

---

## 구성

| 크레이트 | 설명 |
|---|---|
| `uninorm-core` | 핵심 라이브러리 (크로스플랫폼) |
| `uninorm-cli` | CLI 바이너리 |

---

## 라이선스

MIT
