#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import sys
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class PdfObject:
    number: int
    body: bytes


def literal(value: str) -> bytes:
    escaped = value.replace('\\', '\\\\').replace('(', '\\(').replace(')', '\\)')
    return f'({escaped})'.encode('ascii')


def stream(data: bytes, extra: bytes = b'') -> bytes:
    dictionary = b'<< /Length ' + str(len(data)).encode('ascii')
    if extra:
        dictionary += b' ' + extra
    dictionary += b' >>'
    return dictionary + b'\nstream\n' + data + b'\nendstream'


def content(text: str, x: int = 72, y: int = 720) -> bytes:
    return f'BT /F1 18 Tf {x} {y} Td '.encode('ascii') + literal(text) + b' Tj ET'


def write_pdf(path: Path, objects: list[PdfObject], root: int, info: int | None = None) -> dict[str, object]:
    ordered = sorted(objects, key=lambda item: item.number)
    expected = list(range(1, ordered[-1].number + 1))
    actual = [item.number for item in ordered]
    if actual != expected:
        raise ValueError(f'object numbers must be contiguous: {actual}')

    output = bytearray(b'%PDF-1.7\n%\xe2\xe3\xcf\xd3\n')
    offsets = [0]
    for item in ordered:
        offsets.append(len(output))
        output += f'{item.number} 0 obj\n'.encode('ascii')
        output += item.body
        output += b'\nendobj\n'

    xref_offset = len(output)
    output += f'xref\n0 {len(offsets)}\n'.encode('ascii')
    output += b'0000000000 65535 f \n'
    for offset in offsets[1:]:
        output += f'{offset:010d} 00000 n \n'.encode('ascii')

    trailer = f'<< /Size {len(offsets)} /Root {root} 0 R'.encode('ascii')
    if info is not None:
        trailer += f' /Info {info} 0 R'.encode('ascii')
    trailer += b' >>'
    output += b'trailer\n' + trailer + b'\nstartxref\n'
    output += str(xref_offset).encode('ascii') + b'\n%%EOF\n'
    path.write_bytes(output)
    return {
        'path': path.name,
        'bytes': len(output),
        'sha256': hashlib.sha256(output).hexdigest(),
        'objects': len(ordered),
    }


def common_page(page_number: int, parent: int, contents: int, media_box: str = '0 0 612 792', rotate: int | None = None, annots: str | None = None) -> PdfObject:
    body = f'<< /Type /Page /Parent {parent} 0 R /MediaBox [{media_box}] /Resources << /Font << /F1 20 0 R >> >> /Contents {contents} 0 R'.encode('ascii')
    if rotate is not None:
        body += f' /Rotate {rotate}'.encode('ascii')
    if annots is not None:
        body += f' /Annots [{annots}]'.encode('ascii')
    body += b' >>'
    return PdfObject(page_number, body)


def make_plain(path: Path) -> dict[str, object]:
    objects = [
        PdfObject(1, b'<< /Type /Catalog /Pages 2 0 R >>'),
        PdfObject(2, b'<< /Type /Pages /Kids [3 0 R 5 0 R 7 0 R] /Count 3 >>'),
        common_page(3, 2, 4),
        PdfObject(4, stream(content('Plain page 1'))),
        common_page(5, 2, 6, media_box='0 0 595 842'),
        PdfObject(6, stream(content('Plain page 2'))),
        common_page(7, 2, 8, rotate=90),
        PdfObject(8, stream(content('Plain page 3 rotated'))),
        PdfObject(9, b'<< /Title ' + literal('PincerPDF fixture') + b' /Author ' + literal('PincerPDF tests') + b' >>'),
    ]
    objects.extend(PdfObject(number, b'<< >>') for number in range(10, 20))
    objects.append(PdfObject(20, b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>'))
    return write_pdf(path, objects, root=1, info=9)


def make_bookmarks(path: Path) -> dict[str, object]:
    objects = [
        PdfObject(1, b'<< /Type /Catalog /Pages 2 0 R /Outlines 9 0 R /PageMode /UseOutlines >>'),
        PdfObject(2, b'<< /Type /Pages /Kids [3 0 R 5 0 R 7 0 R] /Count 3 >>'),
        common_page(3, 2, 4),
        PdfObject(4, stream(content('Bookmark chapter 1'))),
        common_page(5, 2, 6),
        PdfObject(6, stream(content('Bookmark chapter 2'))),
        common_page(7, 2, 8),
        PdfObject(8, stream(content('Bookmark appendix'))),
        PdfObject(9, b'<< /Type /Outlines /First 10 0 R /Last 11 0 R /Count 2 >>'),
        PdfObject(10, b'<< /Title ' + literal('Chapter 1') + b' /Parent 9 0 R /Next 11 0 R /Dest [3 0 R /Fit] >>'),
        PdfObject(11, b'<< /Title ' + literal('Chapter 2') + b' /Parent 9 0 R /Prev 10 0 R /First 12 0 R /Last 12 0 R /Count 1 /Dest [5 0 R /Fit] >>'),
        PdfObject(12, b'<< /Title ' + literal('Appendix') + b' /Parent 11 0 R /Dest [7 0 R /Fit] >>'),
    ]
    objects.extend(PdfObject(number, b'<< >>') for number in range(13, 20))
    objects.append(PdfObject(20, b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>'))
    return write_pdf(path, objects, root=1)


def make_form(path: Path) -> dict[str, object]:
    objects = [
        PdfObject(1, b'<< /Type /Catalog /Pages 2 0 R /AcroForm 6 0 R >>'),
        PdfObject(2, b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>'),
        common_page(3, 2, 4, annots='7 0 R'),
        PdfObject(4, stream(content('Form fixture'))),
        PdfObject(5, b'<< >>'),
        PdfObject(6, b'<< /Fields [7 0 R] /NeedAppearances true /DA (/F1 12 Tf 0 g) /DR << /Font << /F1 20 0 R >> >> >>'),
        PdfObject(7, b'<< /Type /Annot /Subtype /Widget /FT /Tx /T ' + literal('Name') + b' /V ' + literal('PincerPDF') + b' /Rect [72 660 300 690] /P 3 0 R /F 4 /DA (/F1 12 Tf 0 g) >>'),
    ]
    objects.extend(PdfObject(number, b'<< >>') for number in range(8, 20))
    objects.append(PdfObject(20, b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>'))
    return write_pdf(path, objects, root=1)


def main() -> int:
    output = Path(sys.argv[1] if len(sys.argv) > 1 else 'tests/fixtures/pdf/generated')
    output.mkdir(parents=True, exist_ok=True)
    fixtures = [
        make_plain(output / 'plain-three-pages.pdf'),
        make_bookmarks(output / 'bookmarks.pdf'),
        make_form(output / 'acroform.pdf'),
    ]
    manifest = {'schema': 1, 'fixtures': fixtures}
    (output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
