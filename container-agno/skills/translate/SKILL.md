---
name: translate
description: Translate text between languages. Auto-detects source language. Chinese input translates to English, English input translates to Chinese, other languages translate to Chinese by default. User can specify target language.
allowed-tools: []
---

# Translate

Instant text translation with automatic language detection.

## When to Use

- User sends text prefixed with "翻译", "translate", "译" or similar
- User explicitly asks to translate something
- User pastes foreign-language text and asks what it means

## Instructions

1. **Auto-detect** the source language
2. **Pick target language** using these defaults:
   - Chinese → English
   - English → Chinese
   - Other → Chinese
   - If user specifies a target language, always use that instead
3. **Translate** naturally — not word-for-word. Match the tone (formal/casual) of the original
4. **Respond with only the translation** — no explanations, no "Here is the translation:", no source language label. Just the translated text.

## Examples

User: `翻译 The quick brown fox jumps over the lazy dog`
Response: `敏捷的棕色狐狸跳过了那只懒狗`

User: `translate 今天天气不错，适合出去走走`
Response: `Nice weather today, perfect for a walk outside`

User: `翻译成日语 谢谢你的帮助`
Response: `ご助力いただきありがとうございます`
