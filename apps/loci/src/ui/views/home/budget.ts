import { t } from '@core/i18n/i18n';

/** '6 of 20 min today' once the user has studied today; null before that or without a budget. */
export function budgetLine(studiedSeconds: number | undefined, budgetMinutes: number | undefined): string | null {
  if (!studiedSeconds || studiedSeconds <= 0 || !budgetMinutes || budgetMinutes <= 0) return null;
  const done = Math.round(studiedSeconds / 60);
  return done < 1 ? t('home.hero.budgetUnderMinute', { budget: budgetMinutes }) : t('home.hero.budget', { done, budget: budgetMinutes });
}
