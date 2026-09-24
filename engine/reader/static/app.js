(function () {
  'use strict';

  const state = {
    manifest: null,
    currentEditionId: null,
    loadedEditionId: null,
    currentFile: null,
    loadToken: 0,
    language: (navigator.language || '').toLowerCase().startsWith('zh') ? 'zh' : 'en',
    theme: 'light'
  };

  const strings = {
    en: {
      reader: 'Reader',
      edition: 'Edition',
      repositoryEdition: 'Repository',
      changeEdition: 'Change',
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
      edition: '版本',
      repositoryEdition: '整个仓库',
      changeEdition: '变更',
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
    configureEditionSelector();
    configureDocumentLinks();

    try {
      const response = await fetch('/api/manifest', { cache: 'no-store' });
      if (!response.ok) throw new Error('manifest request failed');
      state.manifest = await response.json();
      renderManifest();
      window.addEventListener('hashchange', onHashChange);
      if (!location.hash) {
        navigateTo(defaultPage(), null, state.currentEditionId);
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
    $('#edition-label').textContent = t('edition');
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

  function configureEditionSelector() {
    $('#edition-selector').addEventListener('change', (event) => {
      const edition = editionById(event.target.value);
      if (!edition) return;
      $('#nav-filter').value = '';
      renderEdition(edition.id);
      navigateTo(defaultPage(edition.id), null, edition.id);
    });
  }

  function renderManifest() {
    const editions = state.manifest.editions || [];
    state.currentEditionId = state.manifest.default_edition || (editions[0] && editions[0].id) || null;
    const selector = $('#edition-selector');
    const picker = $('#edition-picker');
    selector.replaceChildren();
    editions.forEach((edition) => {
      const option = document.createElement('option');
      option.value = edition.id;
      const kindLabel = edition.kind === 'repository' ? t('repositoryEdition') : t('changeEdition');
      option.textContent = kindLabel + ' · ' + String(edition.label || edition.title || edition.id);
      option.title = String(edition.title || edition.label || edition.id);
      selector.appendChild(option);
    });
    picker.hidden = editions.length < 2;
    renderEdition(state.currentEditionId);
  }

  function editionById(editionId) {
    return (state.manifest.editions || []).find((edition) => edition.id === editionId) || null;
  }

  function renderEdition(editionId) {
    const edition = editionById(editionId);
    if (!edition) return false;
    state.currentEditionId = edition.id;
    $('#edition-selector').value = edition.id;
    $('#brand-title').textContent = edition.title || edition.label || 'RepoWiki';
    renderInfo(edition.info || {});
    renderNavigation(edition.navigation || []);
    renderExtraPages(edition.pages || []);
    return true;
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
    link.addEventListener('click', () => navigateTo(node.filename, null, state.currentEditionId));
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
    const extras = pages.slice(1).filter((page) => page.path.length === 0);
    const container = $('#extra-pages');
    const list = $('#extra-pages-list');
    list.replaceChildren();
    extras.forEach((page) => {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'nav-item extra-page';
      button.dataset.file = page.filename;
      button.textContent = page.title;
      button.addEventListener('click', () => navigateTo(page.filename, null, state.currentEditionId));
      list.appendChild(button);
    });
    container.hidden = extras.length === 0;
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

  function defaultPage(editionId) {
    const edition = editionById(editionId || state.currentEditionId);
    const pages = edition ? edition.pages || [] : [];
    const first = pages[0];
    return first ? first.filename : null;
  }

  function buildHash(editionId, filename, anchor) {
    return '#/edition/' + encodeURIComponent(editionId) + '/' + encodeURIComponent(filename) +
      (anchor ? ':' + encodeURIComponent(anchor) : '');
  }

  function navigateTo(filename, anchor, editionId) {
    if (!filename) return;
    const target = buildHash(editionId || state.currentEditionId, filename, anchor);
    if (location.hash === target) onHashChange();
    else location.hash = target;
    $('#sidebar').classList.remove('open');
  }

  function parseHash() {
    const hash = location.hash;
    const defaultEditionId = state.manifest.default_edition || state.currentEditionId;
    if (!hash.startsWith('#/')) {
      return { editionId: defaultEditionId, file: defaultPage(defaultEditionId), anchor: null, legacy: true };
    }

    const editionRoute = hash.startsWith('#/edition/');
    const raw = hash.slice(editionRoute ? '#/edition/'.length : 2);
    let editionId = defaultEditionId;
    let fileAndAnchor = raw;
    if (editionRoute) {
      const separator = raw.indexOf('/');
      if (separator <= 0) return { invalid: true };
      editionId = raw.slice(0, separator);
      fileAndAnchor = raw.slice(separator + 1);
    }
    const anchorSeparator = fileAndAnchor.indexOf(':');
    const filePart = anchorSeparator === -1 ? fileAndAnchor : fileAndAnchor.slice(0, anchorSeparator);
    const anchorPart = anchorSeparator === -1 ? null : fileAndAnchor.slice(anchorSeparator + 1);
    try {
      return {
        editionId: decodeURIComponent(editionId),
        file: decodeURIComponent(filePart),
        anchor: anchorPart ? decodeURIComponent(anchorPart) : null,
        legacy: !editionRoute
      };
    } catch (_) {
      return { invalid: true };
    }
  }

  function onHashChange() {
    const route = parseHash();
    const edition = route.invalid ? null : editionById(route.editionId);
    if (!edition || !route.file) {
      showError(t('loadError'));
      return;
    }
    if (route.legacy) {
      history.replaceState(null, '', location.pathname + location.search +
        buildHash(route.editionId, route.file, route.anchor));
    }
    if (route.editionId !== state.currentEditionId) {
      $('#nav-filter').value = '';
      renderEdition(route.editionId);
    }
    setActiveNavigation(route.file);
    if (route.editionId !== state.loadedEditionId || route.file !== state.currentFile) {
      loadPage(route.editionId, route.file, route.anchor);
    } else if (route.anchor) {
      scrollToAnchor(route.anchor);
    } else {
      window.scrollTo(0, 0);
    }
  }

  async function loadPage(editionId, filename, anchor) {
    const token = ++state.loadToken;
    const edition = editionById(editionId);
    showLoading();
    try {
      const response = await fetch(
        '/api/editions/' + encodeURIComponent(editionId) + '/pages/' + encodeURIComponent(filename),
        { cache: 'no-store' }
      );
      if (!response.ok) throw new Error(t('loadError'));
      const markdown = await response.text();
      if (token !== state.loadToken) return;
      $('#content').innerHTML = renderMarkdown(markdown);
      $('#content').hidden = false;
      $('#loading').hidden = true;
      $('#error').hidden = true;
      state.currentFile = filename;
      state.loadedEditionId = editionId;
      highlightCode();
      renderTOC();
      await renderMermaidDiagrams();
      if (token !== state.loadToken) return;
      renderPager(edition, filename);
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
      link.href = buildHash(state.currentEditionId, state.currentFile, heading.id);
      link.textContent = heading.textContent;
      toc.appendChild(link);
    });
    toc.hidden = false;
  }

  function renderPager(edition, filename) {
    const pager = $('#pager');
    pager.replaceChildren();
    const pages = edition.pages || [];
    const index = pages.findIndex((page) => page.filename === filename);
    if (index === -1 || pages.length < 2) {
      pager.hidden = true;
      return;
    }
    if (index > 0) pager.appendChild(pagerButton(t('previous'), pages[index - 1], 'previous', edition.id));
    if (index + 1 < pages.length) pager.appendChild(pagerButton(t('next'), pages[index + 1], 'next', edition.id));
    pager.hidden = false;
  }

  function pagerButton(label, page, direction, editionId) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'pager-button ' + direction;
    button.textContent = label + ': ' + page.title;
    button.addEventListener('click', () => navigateTo(page.filename, null, editionId));
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
      if (!href || href.startsWith('#/edition/')) return;
      if (href.startsWith('#')) {
        event.preventDefault();
        let anchor = href.slice(1);
        try {
          anchor = decodeURIComponent(anchor);
        } catch (_) {}
        navigateTo(state.currentFile, anchor, state.currentEditionId);
        return;
      }
      const resolved = resolveInternalPage(href);
      if (!resolved) return;
      event.preventDefault();
      navigateTo(resolved.file, resolved.anchor, state.currentEditionId);
    });
  }

  function resolveInternalPage(href) {
    try {
      const url = new URL(href, window.location.origin + '/');
      if (url.origin !== window.location.origin || !url.pathname.endsWith('.md')) return null;
      const filename = decodeURIComponent(url.pathname.replace(/^\/+/, ''));
      const edition = editionById(state.currentEditionId);
      const page = (edition ? edition.pages || [] : []).find((item) => item.filename === filename);
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
