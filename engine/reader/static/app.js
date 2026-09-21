(function () {
  'use strict';

  const state = {
    manifest: null,
    currentFile: null,
    loadToken: 0,
    language: (navigator.language || '').toLowerCase().startsWith('zh') ? 'zh' : 'en',
    theme: 'light'
  };

  const strings = {
    en: {
      reader: 'Reader',
      filter: 'Filter pages',
      otherPages: 'Other pages',
      overview: 'Overview',
      loading: 'Loading documentation…',
      generated: 'Generated',
      model: 'Model',
      commit: 'Commit',
      components: 'Components',
      modules: 'Modules',
      leaves: 'Leaf pages',
      previous: 'Previous',
      next: 'Next',
      onThisPage: 'On this page',
      unavailable: 'Page unavailable',
      loadError: 'Could not load this page.',
      copy: 'Copy code',
      copied: 'Copied',
      openNavigation: 'Open navigation',
      closeNavigation: 'Close navigation',
      lightTheme: 'Use light theme',
      darkTheme: 'Use dark theme',
      diagramError: 'Diagram failed to render'
    },
    zh: {
      reader: '阅读器',
      filter: '筛选页面',
      otherPages: '其他页面',
      overview: '概览',
      loading: '正在加载文档…',
      generated: '生成时间',
      model: '模型',
      commit: '提交',
      components: '组件',
      modules: '模块',
      leaves: '叶页面',
      previous: '上一页',
      next: '下一页',
      onThisPage: '本页目录',
      unavailable: '页面不可用',
      loadError: '无法加载此页面。',
      copy: '复制代码',
      copied: '已复制',
      openNavigation: '打开导航',
      closeNavigation: '关闭导航',
      lightTheme: '切换到浅色主题',
      darkTheme: '切换到深色主题',
      diagramError: '图表渲染失败'
    }
  };

  const $ = (selector) => document.querySelector(selector);
  const t = (key) => strings[state.language][key] || strings.en[key] || key;

  document.addEventListener('DOMContentLoaded', init);

  async function init() {
    applyStaticLabels();
    applyTheme(loadStoredTheme());
    configureMarkdown();
    configureThemeToggle();
    configureMobileMenu();
    configureNavigationFilter();
    configureDocumentLinks();

    try {
      const response = await fetch('/api/manifest', { cache: 'no-store' });
      if (!response.ok) throw new Error('manifest request failed');
      state.manifest = await response.json();
      renderManifest();
      window.addEventListener('hashchange', onHashChange);
      if (!location.hash) {
        navigateTo(defaultPage());
      } else {
        onHashChange();
      }
    } catch (error) {
      showError(error.message || t('loadError'));
    }
  }

  function applyStaticLabels() {
    document.documentElement.lang = state.language === 'zh' ? 'zh-CN' : 'en';
    $('#brand-subtitle').textContent = t('reader');
    $('#search-label').textContent = t('filter');
    $('#nav-filter').placeholder = t('filter');
    $('#loading-text').textContent = t('loading');
    $('#extra-pages-title').textContent = t('otherPages');
    $('#mobile-menu').setAttribute('aria-label', t('openNavigation'));
  }

  function configureMarkdown() {
    if (!window.marked) return;
    marked.setOptions({ gfm: true, breaks: true, html: false });
    if (window.markedGfmHeadingId && markedGfmHeadingId.gfmHeadingId) {
      marked.use(markedGfmHeadingId.gfmHeadingId());
    }
    if (window.hljs && hljs.configure) {
      hljs.configure({ ignoreUnescapedHTML: true });
    }
  }

  function renderManifest() {
    const manifest = state.manifest;
    $('#brand-title').textContent = manifest.title || 'RepoWiki';
    renderInfo(manifest.info || {});
    renderWarnings(manifest.warnings || []);
    renderNavigation(manifest.navigation || []);
    renderExtraPages(manifest.pages || []);
  }

  function renderInfo(info) {
    const rows = [];
    if (info.generated_at) rows.push([t('generated'), formatDate(info.generated_at)]);
    if (info.model) rows.push([t('model'), info.model]);
    if (info.commit) rows.push([t('commit'), info.commit]);
    if (info.total_components != null) rows.push([t('components'), info.total_components.toLocaleString()]);
    if (info.module_count != null) rows.push([t('modules'), info.module_count.toLocaleString()]);
    if (info.leaf_count != null) rows.push([t('leaves'), info.leaf_count.toLocaleString()]);
    const card = $('#info-card');
    card.replaceChildren();
    if (!rows.length) {
      card.hidden = true;
      return;
    }
    rows.forEach(([label, value]) => {
      const row = document.createElement('div');
      row.className = 'info-row';
      const strong = document.createElement('strong');
      strong.textContent = label;
      row.append(strong, document.createTextNode(String(value)));
      card.appendChild(row);
    });
    card.hidden = false;
  }

  function renderWarnings(warnings) {
    const container = $('#warning-list');
    container.replaceChildren();
    if (!warnings.length) {
      container.hidden = true;
      return;
    }
    const title = document.createElement('strong');
    title.textContent = state.language === 'zh' ? '读取提示' : 'Reader warnings';
    container.appendChild(title);
    const list = document.createElement('ul');
    warnings.forEach((warning) => {
      const item = document.createElement('li');
      item.textContent = warning;
      list.appendChild(item);
    });
    container.appendChild(list);
    container.hidden = false;
  }

  function renderNavigation(nodes) {
    const navigation = $('#navigation');
    navigation.replaceChildren();
    nodes.forEach((node) => navigation.appendChild(buildNavNode(node, 0)));
  }

  function buildNavNode(node, depth) {
    const wrapper = document.createElement('div');
    wrapper.className = node.children && node.children.length ? 'nav-group' : 'nav-leaf';
    if (depth > 0 && node.children && node.children.length) wrapper.classList.add('collapsed');

    const row = document.createElement('div');
    row.className = 'nav-row';
    const link = document.createElement('button');
    link.type = 'button';
    link.className = 'nav-item';
    link.dataset.file = node.filename;
    link.textContent = node.name;
    link.disabled = !node.available;
    link.title = node.available ? node.name : t('unavailable');
    link.addEventListener('click', () => navigateTo(node.filename));
    row.appendChild(link);

    if (node.children && node.children.length) {
      const toggle = document.createElement('button');
      toggle.type = 'button';
      toggle.className = 'nav-toggle';
      toggle.textContent = '▸';
      toggle.setAttribute('aria-label', node.name);
      toggle.addEventListener('click', () => wrapper.classList.toggle('collapsed'));
      row.appendChild(toggle);
    }
    wrapper.appendChild(row);

    if (node.children && node.children.length) {
      const children = document.createElement('div');
      children.className = 'nav-children';
      node.children.forEach((child) => children.appendChild(buildNavNode(child, depth + 1)));
      wrapper.appendChild(children);
    }
    return wrapper;
  }

  function renderExtraPages(pages) {
    const knownFiles = new Set();
    flattenNavigation(state.manifest.navigation || []).forEach((node) => knownFiles.add(node.filename));
    const extras = pages.filter((page) => !knownFiles.has(page.filename) && page.filename !== 'overview.md');
    const container = $('#extra-pages');
    const list = $('#extra-pages-list');
    list.replaceChildren();
    extras.forEach((page) => {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'nav-item extra-page';
      button.dataset.file = page.filename;
      button.textContent = page.title;
      button.disabled = !page.available;
      button.addEventListener('click', () => navigateTo(page.filename));
      list.appendChild(button);
    });
    container.hidden = extras.length === 0;
  }

  function flattenNavigation(nodes) {
    const result = [];
    nodes.forEach((node) => {
      result.push(node);
      result.push(...flattenNavigation(node.children || []));
    });
    return result;
  }

  function configureNavigationFilter() {
    $('#nav-filter').addEventListener('input', (event) => {
      const query = event.target.value.trim().toLowerCase();
      document.querySelectorAll('.nav-group, .nav-leaf, .extra-page').forEach((element) => {
        if (!query) {
          element.classList.remove('nav-hidden', 'filter-open');
          return;
        }
        const matches = element.textContent.toLowerCase().includes(query);
        element.classList.toggle('nav-hidden', !matches);
        if (matches && element.classList.contains('nav-group')) {
          element.classList.add('filter-open');
        }
      });
    });
  }

  function configureThemeToggle() {
    $('#theme-toggle').addEventListener('click', () => {
      applyTheme(state.theme === 'dark' ? 'light' : 'dark');
      localStorage.setItem('repowiki-reader-theme', state.theme);
      renderMermaidDiagrams();
    });
  }

  function loadStoredTheme() {
    const stored = localStorage.getItem('repowiki-reader-theme');
    if (stored === 'dark' || stored === 'light') return stored;
    return window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }

  function applyTheme(theme) {
    state.theme = theme;
    document.documentElement.dataset.theme = theme;
    $('#theme-toggle').textContent = theme === 'dark' ? '☀' : '☾';
    $('#theme-toggle').setAttribute('aria-label', theme === 'dark' ? t('lightTheme') : t('darkTheme'));
    $('#hljs-light').disabled = theme === 'dark';
    $('#hljs-dark').disabled = theme !== 'dark';
  }

  function configureMobileMenu() {
    const sidebar = $('#sidebar');
    $('#mobile-menu').addEventListener('click', () => {
      const open = sidebar.classList.toggle('open');
      $('#mobile-menu').setAttribute('aria-label', open ? t('closeNavigation') : t('openNavigation'));
    });
  }

  function defaultPage() {
    const overview = (state.manifest.pages || []).find((page) => page.filename === 'overview.md' && page.available);
    if (overview) return overview.filename;
    const first = (state.manifest.pages || []).find((page) => page.available);
    return first ? first.filename : null;
  }

  function buildHash(filename, anchor) {
    return '#/' + encodeURIComponent(filename) + (anchor ? ':' + encodeURIComponent(anchor) : '');
  }

  function navigateTo(filename, anchor) {
    if (!filename) return;
    const target = buildHash(filename, anchor);
    if (location.hash === target) onHashChange();
    else location.hash = target;
    $('#sidebar').classList.remove('open');
  }

  function parseHash() {
    if (!location.hash.startsWith('#/')) return { file: defaultPage(), anchor: null };
    const raw = location.hash.slice(2);
    const separator = raw.indexOf(':');
    const filePart = separator === -1 ? raw : raw.slice(0, separator);
    const anchorPart = separator === -1 ? null : raw.slice(separator + 1);
    try {
      return {
        file: decodeURIComponent(filePart),
        anchor: anchorPart ? decodeURIComponent(anchorPart) : null
      };
    } catch (_) {
      return { file: defaultPage(), anchor: null };
    }
  }

  function onHashChange() {
    const route = parseHash();
    if (!route.file) {
      showError(t('loadError'));
      return;
    }
    setActiveNavigation(route.file);
    if (route.file === state.currentFile && route.anchor) scrollToAnchor(route.anchor);
    else if (route.file !== state.currentFile) loadPage(route.file, route.anchor);
    else window.scrollTo(0, 0);
  }

  async function loadPage(filename, anchor) {
    const token = ++state.loadToken;
    showLoading();
    try {
      const response = await fetch('/api/pages/' + encodeURIComponent(filename), { cache: 'no-store' });
      if (!response.ok) throw new Error(t('loadError'));
      const markdown = await response.text();
      if (token !== state.loadToken) return;
      $('#content').innerHTML = renderMarkdown(markdown);
      $('#content').hidden = false;
      $('#loading').hidden = true;
      $('#error').hidden = true;
      state.currentFile = filename;
      highlightCode();
      renderTOC();
      await renderMermaidDiagrams();
      renderPager(filename);
      if (anchor) scrollToAnchor(anchor);
      else window.scrollTo(0, 0);
    } catch (error) {
      if (token === state.loadToken) showError(error.message || t('loadError'));
    }
  }

  function renderMarkdown(markdown) {
    if (!window.marked) return '<p>' + escapeHtml(markdown) + '</p>';
    const parsed = marked.parse(markdown);
    const sanitized = window.DOMPurify
      ? DOMPurify.sanitize(parsed, { USE_PROFILES: { html: true } })
      : escapeHtml(markdown);
    const wrapper = document.createElement('div');
    wrapper.innerHTML = sanitized;
    wrapper.querySelectorAll('pre code.language-mermaid').forEach((code) => {
      const diagram = document.createElement('div');
      diagram.className = 'mermaid';
      diagram.dataset.source = code.textContent || '';
      code.closest('pre').replaceWith(diagram);
    });
    return wrapper.innerHTML;
  }

  function highlightCode() {
    document.querySelectorAll('#content pre code:not(.language-mermaid)').forEach((block) => {
      if (window.hljs) hljs.highlightElement(block);
      const pre = block.closest('pre');
      if (!pre || pre.querySelector('.copy-code')) return;
      const button = document.createElement('button');
      button.className = 'copy-code';
      button.type = 'button';
      button.textContent = t('copy');
      button.addEventListener('click', async () => {
        await navigator.clipboard.writeText(block.textContent || '');
        button.textContent = t('copied');
        window.setTimeout(() => { button.textContent = t('copy'); }, 1200);
      });
      pre.appendChild(button);
    });
  }

  async function renderMermaidDiagrams() {
    const diagrams = document.querySelectorAll('#content .mermaid');
    if (!diagrams.length || !window.mermaid) return;
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: 'strict',
      suppressErrorRendering: true,
      theme: state.theme === 'dark' ? 'dark' : 'default',
      flowchart: { htmlLabels: true, curve: 'basis' },
      sequence: { mirrorActors: true, useMaxWidth: true }
    });
    for (let index = 0; index < diagrams.length; index += 1) {
      const element = diagrams[index];
      const source = element.dataset.source || '';
      try {
        const result = await mermaid.render('repowiki-diagram-' + Date.now() + '-' + index, source);
        element.innerHTML = result.svg;
      } catch (error) {
        element.replaceChildren();
        const failure = document.createElement('div');
        failure.className = 'diagram-error';
        failure.textContent = '⚠ ' + t('diagramError');
        const details = document.createElement('details');
        const summary = document.createElement('summary');
        summary.textContent = state.language === 'zh' ? '查看 Mermaid 源码' : 'Show Mermaid source';
        const pre = document.createElement('pre');
        pre.textContent = source;
        details.append(summary, pre);
        element.append(failure, details);
        console.error('Mermaid rendering error', error);
      }
    }
  }

  function renderTOC() {
    const toc = $('#toc');
    toc.replaceChildren();
    const headings = Array.from(document.querySelectorAll('#content h2, #content h3'));
    if (!headings.length) {
      toc.hidden = true;
      return;
    }
    const title = document.createElement('div');
    title.className = 'toc-title';
    title.textContent = t('onThisPage');
    toc.appendChild(title);
    headings.forEach((heading) => {
      if (!heading.id) return;
      const link = document.createElement('a');
      link.className = 'toc-link toc-' + heading.tagName.toLowerCase();
      link.href = '#' + heading.id;
      link.textContent = heading.textContent;
      toc.appendChild(link);
    });
    toc.hidden = false;
  }

  function renderPager(filename) {
    const pager = $('#pager');
    pager.replaceChildren();
    const pages = (state.manifest.pages || []).filter((page) => page.available);
    const index = pages.findIndex((page) => page.filename === filename);
    if (index === -1 || pages.length < 2) {
      pager.hidden = true;
      return;
    }
    if (index > 0) pager.appendChild(pagerButton(t('previous'), pages[index - 1], 'previous'));
    if (index + 1 < pages.length) pager.appendChild(pagerButton(t('next'), pages[index + 1], 'next'));
    pager.hidden = false;
  }

  function pagerButton(label, page, direction) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'pager-button ' + direction;
    button.textContent = label + ': ' + page.title;
    button.addEventListener('click', () => navigateTo(page.filename));
    return button;
  }

  function setActiveNavigation(filename) {
    document.querySelectorAll('[data-file]').forEach((element) => {
      element.classList.toggle('active', element.dataset.file === filename);
    });
    const active = Array.from(document.querySelectorAll('[data-file]'))
      .find((element) => element.dataset.file === filename);
    let group = active && active.closest('.nav-group');
    while (group) {
      group.classList.remove('collapsed');
      group = group.parentElement && group.parentElement.closest('.nav-group');
    }
  }

  function configureDocumentLinks() {
    document.addEventListener('click', (event) => {
      const link = event.target.closest && event.target.closest('a');
      if (!link) return;
      const href = link.getAttribute('href') || '';
      if (!href || href.startsWith('#/')) return;
      if (href.startsWith('#')) {
        event.preventDefault();
        scrollToAnchor(href.slice(1));
        return;
      }
      const resolved = resolveInternalPage(href);
      if (!resolved) return;
      event.preventDefault();
      navigateTo(resolved.file, resolved.anchor);
    });
  }

  function resolveInternalPage(href) {
    try {
      const url = new URL(href, window.location.origin + '/');
      if (url.origin !== window.location.origin || !url.pathname.endsWith('.md')) return null;
      const filename = decodeURIComponent(url.pathname.replace(/^\/+/, ''));
      const page = (state.manifest.pages || []).find((item) => item.filename === filename);
      const anchor = url.hash ? decodeURIComponent(url.hash.slice(1)) : null;
      return page ? { file: filename, anchor } : null;
    } catch (_) {
      return null;
    }
  }

  function scrollToAnchor(anchor) {
    const target = document.getElementById(anchor);
    if (target) target.scrollIntoView({ block: 'start' });
  }

  function showLoading() {
    $('#loading').hidden = false;
    $('#error').hidden = true;
    $('#content').hidden = true;
    $('#pager').hidden = true;
  }

  function showError(message) {
    $('#loading').hidden = true;
    $('#content').hidden = true;
    $('#pager').hidden = true;
    const error = $('#error');
    error.textContent = message || t('loadError');
    error.hidden = false;
  }

  function formatDate(value) {
    const parsed = new Date(value);
    return Number.isNaN(parsed.valueOf()) ? value : parsed.toLocaleString();
  }

  function escapeHtml(value) {
    return String(value).replace(/[&<>"']/g, (character) => ({
      '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
    }[character]));
  }

}());
