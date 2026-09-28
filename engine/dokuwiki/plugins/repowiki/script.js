/*
 * RepoWiki DokuWiki plugin scripts.
 * Copyright (C) 2026 RepoWiki contributors.
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * Full-screen Mermaid diagram viewer: clicking a rendered diagram opens it at
 * inspectable size with wheel zooming and drag panning.
 *
 * This integration is distributed under the GNU GPL, version 2 or (at your
 * option) any later version. See LICENSE for the full text.
 */

(function () {
    'use strict';

    var MIN_SCALE = 0.05;
    var MAX_SCALE = 16;
    var FIT_SCALE_LIMIT = 2;
    var WHEEL_ZOOM = 1.12;
    var DOUBLE_CLICK_ZOOM = 1.6;

    var viewer = null;
    var lastFocused = null;

    function clampScale(value) {
        return Math.min(MAX_SCALE, Math.max(MIN_SCALE, value));
    }

    function naturalSize(svg) {
        var box = svg.viewBox && svg.viewBox.baseVal;
        var width = box && box.width ? box.width : 0;
        var height = box && box.height ? box.height : 0;
        if (!width || !height) {
            var rect = svg.getBoundingClientRect();
            width = rect.width;
            height = rect.height;
        }
        return { width: width || 1, height: height || 1 };
    }

    function applyTransform() {
        viewer.surface.style.transform =
            'translate(' + viewer.x + 'px, ' + viewer.y + 'px) scale(' + viewer.scale + ')';
    }

    function centerView(scale) {
        var stage = viewer.stage.getBoundingClientRect();
        viewer.scale = scale;
        viewer.x = (stage.width - viewer.width * scale) / 2;
        viewer.y = (stage.height - viewer.height * scale) / 2;
        applyTransform();
    }

    function fitScale() {
        var stage = viewer.stage.getBoundingClientRect();
        var availableWidth = Math.max(stage.width - 32, 1);
        var availableHeight = Math.max(stage.height - 32, 1);
        return clampScale(
            Math.min(availableWidth / viewer.width, availableHeight / viewer.height, FIT_SCALE_LIMIT)
        );
    }

    function zoomAt(clientX, clientY, factor) {
        var stage = viewer.stage.getBoundingClientRect();
        var localX = clientX - stage.left;
        var localY = clientY - stage.top;
        var next = clampScale(viewer.scale * factor);
        var ratio = next / viewer.scale;
        viewer.x = localX - (localX - viewer.x) * ratio;
        viewer.y = localY - (localY - viewer.y) * ratio;
        viewer.scale = next;
        applyTransform();
    }

    function closeViewer() {
        if (!viewer) return;
        if (viewer.overlay.parentNode) {
            viewer.overlay.parentNode.removeChild(viewer.overlay);
        }
        viewer = null;
        document.removeEventListener('keydown', onKeyDown, true);
        if (lastFocused && typeof lastFocused.focus === 'function') {
            lastFocused.focus();
        }
        lastFocused = null;
    }

    function onKeyDown(event) {
        if (!viewer) return;
        if (event.key === 'Escape') {
            event.preventDefault();
            closeViewer();
            return;
        }
        if (event.key === '+' || event.key === '=') {
            event.preventDefault();
            centerView(clampScale(viewer.scale * 1.25));
        } else if (event.key === '-' || event.key === '_') {
            event.preventDefault();
            centerView(clampScale(viewer.scale / 1.25));
        } else if (event.key === '0') {
            event.preventDefault();
            centerView(fitScale());
        }
    }

    function buildViewer(svg, size) {
        var overlay = document.createElement('div');
        overlay.className = 'repowiki-viewer';
        overlay.setAttribute('role', 'dialog');
        overlay.setAttribute('aria-modal', 'true');
        overlay.setAttribute('aria-label', 'Diagram viewer');

        var bar = document.createElement('div');
        bar.className = 'repowiki-viewer-bar';

        var actions = [
            { action: 'zoom-out', label: 'Zoom out', text: '−' },
            { action: 'zoom-in', label: 'Zoom in', text: '+' },
            { action: 'reset', label: 'Reset view', text: 'Reset' },
            { action: 'close', label: 'Close', text: '×' }
        ];
        var buttons = {};
        actions.forEach(function (entry) {
            var button = document.createElement('button');
            button.type = 'button';
            button.setAttribute('data-viewer-action', entry.action);
            button.setAttribute('aria-label', entry.label);
            button.title = entry.label;
            button.textContent = entry.text;
            bar.appendChild(button);
            buttons[entry.action] = button;
        });

        var stage = document.createElement('div');
        stage.className = 'repowiki-viewer-stage';

        var surface = document.createElement('div');
        surface.className = 'repowiki-viewer-surface';
        surface.style.width = size.width + 'px';
        surface.style.height = size.height + 'px';

        var clone = svg.cloneNode(true);
        clone.style.width = size.width + 'px';
        clone.style.height = size.height + 'px';
        clone.style.maxWidth = 'none';
        clone.removeAttribute('aria-labelledby');
        surface.appendChild(clone);
        stage.appendChild(surface);
        overlay.appendChild(bar);
        overlay.appendChild(stage);
        return { overlay: overlay, stage: stage, surface: surface, buttons: buttons };
    }

    function openViewer(svg) {
        if (viewer) return;
        var size = naturalSize(svg);
        var parts = buildViewer(svg, size);
        lastFocused = document.activeElement;

        viewer = {
            overlay: parts.overlay,
            stage: parts.stage,
            surface: parts.surface,
            width: size.width,
            height: size.height,
            scale: 1,
            x: 0,
            y: 0,
            drag: null
        };

        parts.buttons['zoom-in'].addEventListener('click', function () {
            centerView(clampScale(viewer.scale * 1.25));
        });
        parts.buttons['zoom-out'].addEventListener('click', function () {
            centerView(clampScale(viewer.scale / 1.25));
        });
        parts.buttons['reset'].addEventListener('click', function () {
            centerView(fitScale());
        });
        parts.buttons['close'].addEventListener('click', closeViewer);

        parts.stage.addEventListener('wheel', function (event) {
            event.preventDefault();
            zoomAt(event.clientX, event.clientY, event.deltaY < 0 ? WHEEL_ZOOM : 1 / WHEEL_ZOOM);
        }, { passive: false });

        parts.stage.addEventListener('dblclick', function (event) {
            zoomAt(event.clientX, event.clientY, DOUBLE_CLICK_ZOOM);
        });

        parts.stage.addEventListener('pointerdown', function (event) {
            if (event.button !== 0) return;
            viewer.drag = {
                x: event.clientX,
                y: event.clientY,
                originX: viewer.x,
                originY: viewer.y,
                moved: false
            };
            parts.stage.classList.add('is-dragging');
            if (typeof parts.stage.setPointerCapture === 'function') {
                parts.stage.setPointerCapture(event.pointerId);
            }
        });

        parts.stage.addEventListener('pointermove', function (event) {
            if (!viewer.drag) return;
            var dx = event.clientX - viewer.drag.x;
            var dy = event.clientY - viewer.drag.y;
            if (Math.abs(dx) > 3 || Math.abs(dy) > 3) viewer.drag.moved = true;
            viewer.x = viewer.drag.originX + dx;
            viewer.y = viewer.drag.originY + dy;
            applyTransform();
        });

        parts.stage.addEventListener('pointerup', function (event) {
            if (!viewer.drag) return;
            var moved = viewer.drag.moved;
            viewer.drag = null;
            parts.stage.classList.remove('is-dragging');
            if (!moved && event.target === parts.stage) closeViewer();
        });

        document.addEventListener('keydown', onKeyDown, true);
        document.body.appendChild(parts.overlay);
        centerView(fitScale());
        parts.buttons['close'].focus();
    }

    document.addEventListener('click', function (event) {
        var target = event.target;
        if (!target || typeof target.closest !== 'function') return;
        if (target.closest('.repowiki-viewer')) return;
        var svg = target.closest('.mermaid svg');
        if (!svg) return;
        event.preventDefault();
        openViewer(svg);
    });
})();
