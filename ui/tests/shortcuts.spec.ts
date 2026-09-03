import { test, expect } from '@playwright/test';
import { shortcutKeyFor } from '../src/lib/shortcuts';

test.describe('shortcutKeyFor', () => {
  test('a Latin layout is read by what it writes, lowercased', () => {
    expect(shortcutKeyFor('h', 'KeyH')).toBe('h');
    expect(shortcutKeyFor('H', 'KeyH')).toBe('h');
    expect(shortcutKeyFor('1', 'Digit1')).toBe('1');
    expect(shortcutKeyFor('1', 'Numpad1')).toBe('1');
    expect(shortcutKeyFor('?', 'Slash')).toBe('?');
    expect(shortcutKeyFor('Enter', 'Enter')).toBe('enter');
    expect(shortcutKeyFor('Backspace', 'Backspace')).toBe('backspace');
  });

  test('a layout with its own script is read by the physical key (#38)', () => {
    // Persian: the top row writes Extended Arabic-Indic digits.
    expect(shortcutKeyFor('۱', 'Digit1')).toBe('1');
    expect(shortcutKeyFor('۵', 'Digit5')).toBe('5');
    // Bulgarian phonetic: the key under H writes х.
    expect(shortcutKeyFor('х', 'KeyH')).toBe('h');
    expect(shortcutKeyFor('Х', 'KeyH')).toBe('h');
    // Greek, Cyrillic, Arabic — any single non-ASCII character.
    expect(shortcutKeyFor('λ', 'KeyL')).toBe('l');
  });

  test('a Latin layout with its own arrangement keeps what it writes', () => {
    // Dvorak: the physical H key writes d, and d is what was meant.
    expect(shortcutKeyFor('d', 'KeyH')).toBe('d');
    expect(shortcutKeyFor('j', 'KeyC')).toBe('j');
  });

  test('a non-ASCII key on a code the tables do not know stays as written', () => {
    expect(shortcutKeyFor('ж', 'Semicolon')).toBe('ж');
    expect(shortcutKeyFor('€', 'Digit5')).toBe('5');
    expect(shortcutKeyFor('ß', '')).toBe('ß');
  });
});
