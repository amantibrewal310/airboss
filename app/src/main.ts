import { mount } from 'svelte';
import App from './windows/main/App.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('missing #app');
mount(App, { target });
