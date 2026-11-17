/** Provider logo URLs: the SVGs in `assets/logos/providers`, one import each. */
import alibaba from "../../../../assets/logos/providers/alibaba.svg";
import anthropic from "../../../../assets/logos/providers/anthropic.svg";
import cohere from "../../../../assets/logos/providers/cohere.svg";
import deepseek from "../../../../assets/logos/providers/deepseek.svg";
import google from "../../../../assets/logos/providers/google.svg";
import meta from "../../../../assets/logos/providers/meta.svg";
import minimax from "../../../../assets/logos/providers/minimax.svg";
import mistral from "../../../../assets/logos/providers/mistral.svg";
import moonshotai from "../../../../assets/logos/providers/moonshotai.svg";
import nvidia from "../../../../assets/logos/providers/nvidia.svg";
import openai from "../../../../assets/logos/providers/openai.svg";
import xai from "../../../../assets/logos/providers/xai.svg";
import zai from "../../../../assets/logos/providers/zai.svg";

/** Logo URL per `ModelProvider.id`. */
export const PROVIDER_LOGO: ReadonlyMap<string, string> = new Map([
  ["alibaba", alibaba],
  ["anthropic", anthropic],
  ["cohere", cohere],
  ["deepseek", deepseek],
  ["google", google],
  ["meta", meta],
  ["minimax", minimax],
  ["mistral", mistral],
  ["moonshotai", moonshotai],
  ["nvidia", nvidia],
  ["openai", openai],
  ["xai", xai],
  ["zai", zai],
]);
