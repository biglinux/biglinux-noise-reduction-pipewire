"""Exact-match, fail-closed patch helpers for the temporary review branch."""
from pathlib import Path
import subprocess


def done(title: str) -> bool:
    messages = subprocess.check_output(['git', 'log', '--format=%s', '-100'], text=True).splitlines()
    return title in messages


def replace(path: str, old: str, new: str, count: int = 1) -> None:
    file = Path(path)
    content = file.read_text(encoding='utf-8')
    found = content.count(old)
    if found != count:
        raise RuntimeError(f'{path}: expected {count} matches, found {found}: {old[:120]!r}')
    file.write_text(content.replace(old, new), encoding='utf-8')


def write(path: str, content: str) -> None:
    file = Path(path)
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(content, encoding='utf-8')


def commit(title: str, paths: list[str]) -> None:
    subprocess.run(['git', 'diff', '--check'], check=True)
    subprocess.run(['git', 'add', '--', *paths], check=True)
    subprocess.run(['git', 'commit', '-m', title], check=True)
