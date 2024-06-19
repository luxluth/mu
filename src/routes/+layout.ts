import type { LayoutLoad } from './$types';
import { _init } from '$lib/i18n';
import { locale } from 'svelte-i18n';

export const prerender = true;
export const ssr = false;

export const load: LayoutLoad = async ({ route }) => {
	const lang = 'fr'; // await invoke<string>('locale');
	console.log('[INFO::locale]', lang);
	_init(lang);
	locale.set(lang);

	return {
		route: route.id as string,
		lang,
		platform: 'linux' // await invoke<string>('platform')
	};
};
