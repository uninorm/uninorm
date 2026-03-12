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

# 디렉토리 감시 (데몬 자동 시작)
uninorm watch add ~/Downloads

# 클립보드 텍스트 변환
uninorm clipboard

# 텍스트 NFD → NFC 변환 (텍스트 생략 시 stdin에서 읽기)
echo "NFD 텍스트" | uninorm convert
```

전체 CLI 레퍼런스는 [docs/cli.ko.md](docs/cli.ko.md)를 참고하세요.

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
