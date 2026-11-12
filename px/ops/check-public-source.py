"""Reject private deployment material in files tracked by the public fork."""
import ipaddress
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[2]
PUBLIC_HOSTS = {'github.com', 'api.github.com', 'developers.openai.com',
                'platform.claude.com', 'support.claude.com', 'learn.chatgpt.com',
                'chatgpt.com', 'api.openai.com', 'chai.aihao666.com'}


def violations(name, data):
    errors = []
    if name.startswith(('private/', 'backups/', 'evidence/')) or name.endswith(('.db', '.sqlite', '.pem', '.key')):
        errors.append('private file type or directory')
    if name.startswith('px/') and re.search(rb'-----BEGIN (?:OPENSSH |RSA |EC )?PRIVATE KEY-----\r?\n[A-Za-z0-9+/=\r\n]{64,}-----END', data):
        errors.append('private key material')
    if name.startswith('px/'):
        for raw in re.findall(rb'(?:https?|wss?|ssh)://[^\s<>"\x27`]+', data):
            try:
                url = urlsplit(raw.decode())
                host = (url.hostname or '').lower()
            except (ValueError, UnicodeDecodeError):
                continue
            if url.username or url.password:
                errors.append('URL credentials')
            if not host or host in PUBLIC_HOSTS or host in ['localhost', '127.0.0.1', '0.0.0.0'] or host.endswith(('.example', '.invalid', '.localhost', '.example.com', '.example.org', '.example.net')) or host in ['example.com', 'example.org', 'example.net']:
                continue
            errors.append('unreviewed deployment URL')
        for address in re.findall(rb'(?<![\w.])(?:\d{1,3}\.){3}\d{1,3}(?![\w.])', data):
            try:
                ip = ipaddress.ip_address(address.decode())
            except ValueError:
                continue
            documented = any(ip in ipaddress.ip_network(network) for network in ['192.0.2.0/24', '198.51.100.0/24', '203.0.113.0/24'])
            if not ip.is_loopback and not ip.is_unspecified and not documented:
                errors.append('deployment IP address')
        if re.search(rb'(?i)(?:[a-z]:[/\\]users[/\\]|/home/)(?!example[/\\]|developer[/\\]|runner[/\\]|test[/\\])[a-z0-9_.-]{1,64}[/\\]', data):
            errors.append('personal home directory')
        if re.search(rb'(?i)\b[a-z0-9._%+-]+@(?!example\.(?:com|org|net)|[^\s@]+\.(?:invalid|example)\b|localhost\b)[a-z0-9.-]+\.[a-z]{2,}\b', data):
            errors.append('personal email address')
    return sorted(set(errors))


def main():
    names = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT).decode().split('\0')
    failed = []
    for name in filter(None, names):
        file = ROOT / name
        if file.is_file():
            for reason in violations(name, file.read_bytes()):
                failed.append((name, reason))
    for name, reason in failed:
        print(f'{name}: {reason}')
    if failed:
        raise SystemExit(1)
    print('Public-source privacy checks passed; no deployment values printed.')


if __name__ == '__main__':
    main()
