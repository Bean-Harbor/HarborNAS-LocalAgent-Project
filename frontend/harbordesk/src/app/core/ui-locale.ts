export function prefersChineseUi(): boolean {
  const localeOverride = new URLSearchParams(globalThis.location?.search ?? '').get('locale')
    ?? new URLSearchParams(globalThis.location?.search ?? '').get('lang');
  if (localeOverride?.toLowerCase().startsWith('en')) {
    return false;
  }
  if (localeOverride?.toLowerCase().startsWith('zh')) {
    return true;
  }

  const navigatorLike = globalThis.navigator;
  const languages = [
    ...(navigatorLike?.languages ?? []),
    navigatorLike?.language
  ].filter((value): value is string => Boolean(value));

  if (languages.some((language) => language.toLowerCase().startsWith('zh'))) {
    return true;
  }

  return true;
}

export function uiText(english: string, chinese: string): string {
  return prefersChineseUi() ? chinese : english;
}
