import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('public_source', Path(__file__).with_name('check-public-source.py'))
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class PublicSourceTests(unittest.TestCase):
    def test_rejects_unreviewed_hosts_without_echoing_the_value(self):
        private = b'https://' + b'private-host.customer.test.example.edu/admin'
        self.assertEqual(guard.violations('px/example.md', private), ['unreviewed deployment URL'])

    def test_allows_documentation_addresses_and_upstream_endpoints(self):
        values = b'https://gateway.example.com/comments wss://chatgpt.com/backend-api/codex/responses 192.0.2.10 127.0.0.1'
        self.assertEqual(guard.violations('px/example.md', values), [])

    def test_rejects_private_files_and_url_credentials(self):
        self.assertIn('private file type or directory', guard.violations('private/auth.json', b'{}'))
        self.assertIn('URL credentials', guard.violations('px/example.md', b'https://' + b'user:password@gateway.example.com'))


if __name__ == '__main__':
    unittest.main()
