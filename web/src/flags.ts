// National flags (flag-icons, 4:3 SVGs). Each is its own file under
// /assets/, fetched only when a page shows that country.

const FLAGS = import.meta.glob<string>("/node_modules/flag-icons/flags/4x3/*.svg", {
  eager: true,
  query: "?url",
  import: "default",
});

/** The flag image for a two-letter country code ("SE"), if there is one. */
export function flagUrl(country: string | undefined): string | undefined {
  if (!country || !/^[A-Za-z]{2}$/.test(country)) return undefined;
  return FLAGS[`/node_modules/flag-icons/flags/4x3/${country.toLowerCase()}.svg`];
}
