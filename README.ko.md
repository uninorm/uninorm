# mac-uninorm

macOS의 NFD 파일명과 텍스트를 NFC로 변환합니다.

macOS HFS+/APFS는 파일명을 비표준 NFD로 저장해서, 한글·일본어 카나·라틴 악센트 문자가 Linux/Windows에서 깨집니다.

> English: [README.md](README.md)

## 설치

```bash
cargo install --path crates/uninorm-cli
```

## 사용법

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

## 동작 원리

macOS는 파일명을 쓸 때 `강` (U+AC15)을 낱자(`ᄀ` + `ᅡ` + `ᆼ`)로 분해합니다. 일본어 탁점 카나(`が` → `か` + `゛`), 라틴 악센트(`é` → `e` + `´`)도 마찬가지입니다.

`uninorm`은 이를 다른 시스템이 기대하는 NFC 완성형으로 다시 합칩니다.

> **참고:** macOS의 HFS+ NFD는 Unicode 표준 NFD와 다릅니다. `uninorm`은 두 변형을 모두 올바르게 처리합니다.

## 구성

| 크레이트 | 설명 |
|---|---|
| `uninorm-core` | 핵심 라이브러리 (크로스플랫폼) |
| `uninorm-cli` | CLI 바이너리 |

## 라이선스

MIT
