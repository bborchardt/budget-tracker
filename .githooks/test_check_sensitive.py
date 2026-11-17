"""Tests for check-sensitive.py's content rules.

Run: python3 -I .githooks/test_check_sensitive.py

Sample values are assembled at runtime so this file never contains a literal that the
hook itself would flag.
"""
import importlib.util
import sys
import pathlib
import unittest

sys.dont_write_bytecode = True
_path = pathlib.Path(__file__).with_name("check-sensitive.py")
_spec = importlib.util.spec_from_file_location("check_sensitive", _path)
cs = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(cs)


def luhn_number(prefix: str, length: int = 16) -> str:
    """A made-up number of the given length that passes the Luhn check."""
    body = (prefix + "0" * length)[: length - 1]
    for check in "0123456789":
        if cs.luhn_ok(body + check):
            return body + check
    raise AssertionError("unreachable")


CARD = luhn_number("4")
AT = "@"


class CardNumber(unittest.TestCase):
    def test_flags_plain_and_grouped(self):
        self.assertTrue(cs.card_number(f"card {CARD} here"))
        grouped = " ".join(CARD[i:i + 4] for i in range(0, 16, 4))
        self.assertTrue(cs.card_number(grouped))
        self.assertTrue(cs.card_number(grouped.replace(" ", "-")))
        self.assertTrue(cs.card_number(f"x,{CARD},y"))
        self.assertTrue(cs.card_number(f'"{CARD}"'))

    def test_skips_digits_inside_hex_checksum(self):
        self.assertTrue(cs.luhn_ok(CARD))
        self.assertFalse(cs.card_number(f'checksum = "ab{CARD}cd"'))
        self.assertFalse(cs.card_number(f"sha {CARD}f00d"))

    def test_skips_non_luhn(self):
        bad = CARD[:-1] + str((int(CARD[-1]) + 1) % 10)
        self.assertFalse(cs.card_number(bad))


class Email(unittest.TestCase):
    def test_flags_real_looking_addresses(self):
        self.assertTrue(cs.email_address("contact someone" + AT + "mailhost.com"))
        self.assertTrue(cs.email_address("a.b" + AT + "sub.mailhost.co"))

    def test_allows_example_domains(self):
        self.assertFalse(cs.email_address("someone" + AT + "example.com"))

    def test_skips_filenames(self):
        self.assertFalse(cs.email_address('"icons/128x128' + AT + '2x.png",'))
        self.assertFalse(cs.email_address("logo" + AT + "3x.svg"))

    def test_filename_does_not_hide_a_real_address(self):
        line = "icon" + AT + "2x.png and someone" + AT + "mailhost.com"
        self.assertTrue(cs.email_address(line))


if __name__ == "__main__":
    unittest.main()
