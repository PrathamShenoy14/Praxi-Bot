import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { repliesToSpeak } from "../lib/autoSpeak";
import { speak, speechSupported } from "../lib/speaker";
import { markdownToSpeech } from "../lib/speechText";
import { DEFAULT_SETTINGS, loadSettingsSync } from "../store/settingsStore";
import type { RunState } from "../state/timeline";

/** Reads a finished reply aloud when the owner turned "Speak replies" on (settings mirror, read live). */
export function useAutoSpeak(chatId: string, runs: readonly RunState[], external = false): void {
  const { t, i18n } = useTranslation();
  const seen = useRef(new Set<string>());
  const primedFor = useRef<string | null>(null);
  useEffect(() => {
    if (external) {
      primedFor.current = null;
      return;
    }
    const prime = primedFor.current !== chatId;
    primedFor.current = chatId;
    if (prime) {
      seen.current = new Set();
    }
    const due = repliesToSpeak(runs, seen.current, prime);
    const prefs = loadSettingsSync() ?? DEFAULT_SETTINGS;
    const last = due[due.length - 1];
    if (!last || !prefs.speakReplies || !speechSupported()) {
      return;
    }
    speak(
      `auto:${last.id}`,
      markdownToSpeech(last.text, { code: t("voice.codeOmitted"), table: t("voice.tableOmitted") }),
      { voiceName: prefs.voiceName, rate: prefs.voiceRate, lang: i18n.language }
    );
  }, [chatId, runs, t, i18n.language, external]);
}
