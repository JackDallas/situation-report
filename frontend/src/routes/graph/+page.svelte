<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import {
		forceSimulation,
		forceLink,
		forceManyBody,
		forceCenter,
		forceCollide,
		type SimulationNodeDatum,
		type SimulationLinkDatum,
	} from 'd3-force';
	import { api } from '$lib/services/api';
	import type { EntityDetailResponse } from '$lib/services/api';

	// --- Types ---

	interface GraphNode extends SimulationNodeDatum {
		id: string;
		canonical_name: string;
		entity_type: string;
		mention_count: number;
		aliases: string[];
		wikidata_id: string | null;
		confidence: number;
		radius: number;
	}

	interface GraphLink extends SimulationLinkDatum<GraphNode> {
		id: string;
		relationship: string;
		confidence: number;
		evidence_count: number;
		source_entity: string;
		target_entity: string;
	}

	// --- Constants ---

	const ENTITY_COLORS: Record<string, string> = {
		person: '#3b82f6',
		location: '#10b981',
		organization: '#f97316',
		weapon_system: '#ef4444',
		military_unit: '#8b5cf6',
		facility: '#a855f7',
	};

	const RELATIONSHIP_COLORS: Record<string, string> = {
		rivalry: '#ef4444',
		alliance: '#10b981',
		leadership: '#3b82f6',
		membership: '#60a5fa',
		geographic_association: '#6b7280',
		supply_chain: '#f59e0b',
		family: '#ec4899',
		sponsorship: '#8b5cf6',
	};

	const DASHED_RELATIONSHIPS = new Set([
		'geographic_association',
		'supply_chain',
		'sponsorship',
	]);

	// --- State ---

	let canvas: HTMLCanvasElement | undefined = $state();
	let container: HTMLDivElement | undefined = $state();
	let nodes = $state<GraphNode[]>([]);
	let links = $state<GraphLink[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let hoveredNode = $state<GraphNode | null>(null);
	let selectedNode = $state<GraphNode | null>(null);
	let selectedDetail = $state<EntityDetailResponse | null>(null);
	let detailLoading = $state(false);

	// Transform state
	let transform = $state({ x: 0, y: 0, k: 1 });
	let isDragging = $state(false);
	let dragStart = $state({ x: 0, y: 0 });
	let dragNode = $state<GraphNode | null>(null);

	let simulation: ReturnType<typeof forceSimulation<GraphNode>> | null = null;
	let animFrame: number | null = null;
	let width = $state(0);
	let height = $state(0);

	// --- Data loading ---

	async function loadGraph() {
		loading = true;
		error = null;
		try {
			const entities = await api.getEntities(150);

			if (Array.isArray(entities) && entities.length > 0) {
				// Build nodes
				const maxMentions = Math.max(...entities.map((e) => e.mention_count));
				const nodeMap = new Map<string, GraphNode>();

				for (const e of entities) {
					const node: GraphNode = {
						id: e.id,
						canonical_name: e.canonical_name,
						entity_type: e.entity_type,
						mention_count: e.mention_count,
						aliases: e.aliases ?? [],
						wikidata_id: e.wikidata_id,
						confidence: e.confidence,
						radius: 6 + (e.mention_count / Math.max(maxMentions, 1)) * 20,
					};
					nodeMap.set(e.id, node);
				}

				// Fetch relationships for top entities (batch)
				const allLinks: GraphLink[] = [];
				const seenLinks = new Set<string>();

				// Get details for top 50 entities to gather relationships
				const topEntities = entities.slice(0, 50);
				const detailPromises = topEntities.map((e) =>
					api.getEntityDetail(e.id).catch(() => null),
				);
				const details = await Promise.all(detailPromises);

				for (const detail of details) {
					if (!detail?.relationships) continue;
					for (const rel of detail.relationships) {
						const linkKey = [rel.source_entity, rel.target_entity, rel.relationship]
							.sort()
							.join('::');
						if (seenLinks.has(linkKey)) continue;
						if (!nodeMap.has(rel.source_entity) || !nodeMap.has(rel.target_entity))
							continue;
						seenLinks.add(linkKey);
						allLinks.push({
							id: rel.id,
							source: rel.source_entity,
							target: rel.target_entity,
							relationship: rel.relationship,
							confidence: rel.confidence,
							evidence_count: rel.evidence_count,
							source_entity: rel.source_entity,
							target_entity: rel.target_entity,
						});
					}
				}

				nodes = Array.from(nodeMap.values());
				links = allLinks;
				startSimulation();
			} else {
				nodes = [];
				links = [];
			}
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load entity graph';
		} finally {
			loading = false;
		}
	}

	// --- Simulation ---

	function startSimulation() {
		if (simulation) simulation.stop();

		simulation = forceSimulation<GraphNode>(nodes)
			.force(
				'link',
				forceLink<GraphNode, GraphLink>(links)
					.id((d) => d.id)
					.distance(100),
			)
			.force('charge', forceManyBody().strength(-200))
			.force('center', forceCenter(width / 2, height / 2))
			.force(
				'collide',
				forceCollide<GraphNode>().radius((d) => d.radius + 4),
			)
			.alphaDecay(0.02)
			.on('tick', () => draw());
	}

	// --- Canvas rendering ---

	function draw() {
		if (!canvas) return;
		const ctx = canvas.getContext('2d');
		if (!ctx) return;

		const dpr = window.devicePixelRatio || 1;
		ctx.clearRect(0, 0, canvas.width, canvas.height);
		ctx.save();
		ctx.scale(dpr, dpr);
		ctx.translate(transform.x, transform.y);
		ctx.scale(transform.k, transform.k);

		// Draw links
		for (const link of links) {
			const source = link.source as GraphNode;
			const target = link.target as GraphNode;
			if (source.x == null || source.y == null || target.x == null || target.y == null)
				continue;

			const isHighlighted =
				hoveredNode &&
				(source.id === hoveredNode.id || target.id === hoveredNode.id);
			const isSelectedEdge =
				selectedNode &&
				(source.id === selectedNode.id || target.id === selectedNode.id);

			ctx.beginPath();
			ctx.strokeStyle =
				isHighlighted || isSelectedEdge
					? RELATIONSHIP_COLORS[link.relationship] ?? '#6b7280'
					: hoveredNode || selectedNode
						? 'rgba(55, 65, 81, 0.15)'
						: (RELATIONSHIP_COLORS[link.relationship] ?? '#6b7280') + '60';
			ctx.lineWidth = isHighlighted || isSelectedEdge ? 2 : 1;

			if (DASHED_RELATIONSHIPS.has(link.relationship)) {
				ctx.setLineDash([4, 4]);
			} else {
				ctx.setLineDash([]);
			}

			ctx.moveTo(source.x, source.y);
			ctx.lineTo(target.x, target.y);
			ctx.stroke();
			ctx.setLineDash([]);
		}

		// Draw nodes
		for (const node of nodes) {
			if (node.x == null || node.y == null) continue;

			const isHovered = hoveredNode?.id === node.id;
			const isSelected = selectedNode?.id === node.id;
			const hovId = hoveredNode?.id;
			const isConnected =
				hovId != null &&
				links.some((l) => {
					const s = (l.source as GraphNode).id;
					const t = (l.target as GraphNode).id;
					return (
						(s === hovId && t === node.id) ||
						(t === hovId && s === node.id)
					);
				});
			const selId = selectedNode?.id;
			const isSelectedConnected =
				selId != null &&
				links.some((l) => {
					const s = (l.source as GraphNode).id;
					const t = (l.target as GraphNode).id;
					return (
						(s === selId && t === node.id) ||
						(t === selId && s === node.id)
					);
				});

			const dimmed =
				(hoveredNode && !isHovered && !isConnected) ||
				(selectedNode && !isSelected && !isSelectedConnected);

			const color = ENTITY_COLORS[node.entity_type] ?? '#6b7280';

			// Glow for hovered/selected
			if (isHovered || isSelected) {
				ctx.beginPath();
				ctx.arc(node.x, node.y, node.radius + 4, 0, Math.PI * 2);
				ctx.fillStyle = color + '30';
				ctx.fill();
			}

			// Node circle
			ctx.beginPath();
			ctx.arc(node.x, node.y, node.radius, 0, Math.PI * 2);
			ctx.fillStyle = dimmed ? color + '30' : color;
			ctx.fill();

			if (isHovered || isSelected) {
				ctx.strokeStyle = '#ffffff';
				ctx.lineWidth = 2;
				ctx.stroke();
			}

			// Label
			const fontSize = Math.max(9, Math.min(12, node.radius * 0.9));
			ctx.font = `${fontSize}px Inter, sans-serif`;
			ctx.textAlign = 'center';
			ctx.textBaseline = 'top';
			ctx.fillStyle = dimmed ? 'rgba(148, 163, 184, 0.25)' : 'rgba(226, 232, 240, 0.9)';
			ctx.fillText(node.canonical_name, node.x, node.y + node.radius + 4, 120);
		}

		ctx.restore();
	}

	// --- Interaction helpers ---

	function screenToWorld(sx: number, sy: number): { x: number; y: number } {
		return {
			x: (sx - transform.x) / transform.k,
			y: (sy - transform.y) / transform.k,
		};
	}

	function findNodeAt(wx: number, wy: number): GraphNode | null {
		// Iterate in reverse so top-drawn nodes are hit first
		for (let i = nodes.length - 1; i >= 0; i--) {
			const n = nodes[i];
			if (n === undefined || n.x == null || n.y == null) continue;
			const dx = wx - n.x;
			const dy = wy - n.y;
			if (dx * dx + dy * dy <= (n.radius + 4) * (n.radius + 4)) {
				return n;
			}
		}
		return null;
	}

	function handleMouseMove(e: MouseEvent) {
		if (!canvas) return;
		const rect = canvas.getBoundingClientRect();
		const sx = e.clientX - rect.left;
		const sy = e.clientY - rect.top;
		const { x: wx, y: wy } = screenToWorld(sx, sy);

		if (dragNode) {
			// Dragging a node
			dragNode.fx = wx;
			dragNode.fy = wy;
			simulation?.alpha(0.3).restart();
			return;
		}

		if (isDragging) {
			// Panning
			transform = {
				...transform,
				x: transform.x + (e.clientX - dragStart.x),
				y: transform.y + (e.clientY - dragStart.y),
			};
			dragStart = { x: e.clientX, y: e.clientY };
			draw();
			return;
		}

		const node = findNodeAt(wx, wy);
		hoveredNode = node;
		if (canvas) canvas.style.cursor = node ? 'pointer' : 'grab';
		draw();
	}

	function handleMouseDown(e: MouseEvent) {
		if (!canvas) return;
		const rect = canvas.getBoundingClientRect();
		const sx = e.clientX - rect.left;
		const sy = e.clientY - rect.top;
		const { x: wx, y: wy } = screenToWorld(sx, sy);

		const node = findNodeAt(wx, wy);
		if (node) {
			dragNode = node;
			dragNode.fx = node.x;
			dragNode.fy = node.y;
			simulation?.alphaTarget(0.3).restart();
		} else {
			isDragging = true;
			dragStart = { x: e.clientX, y: e.clientY };
			if (canvas) canvas.style.cursor = 'grabbing';
		}
	}

	function handleMouseUp(_e: MouseEvent) {
		if (dragNode) {
			dragNode.fx = null;
			dragNode.fy = null;
			simulation?.alphaTarget(0);
			dragNode = null;
		}
		isDragging = false;
		if (canvas) canvas.style.cursor = hoveredNode ? 'pointer' : 'grab';
	}

	async function handleClick(e: MouseEvent) {
		if (!canvas) return;
		const rect = canvas.getBoundingClientRect();
		const sx = e.clientX - rect.left;
		const sy = e.clientY - rect.top;
		const { x: wx, y: wy } = screenToWorld(sx, sy);

		const node = findNodeAt(wx, wy);
		if (node) {
			selectedNode = node;
			detailLoading = true;
			selectedDetail = null;
			try {
				selectedDetail = await api.getEntityDetail(node.id);
			} catch {
				selectedDetail = null;
			} finally {
				detailLoading = false;
			}
		} else {
			selectedNode = null;
			selectedDetail = null;
		}
		draw();
	}

	function handleWheel(e: WheelEvent) {
		e.preventDefault();
		if (!canvas) return;
		const rect = canvas.getBoundingClientRect();
		const sx = e.clientX - rect.left;
		const sy = e.clientY - rect.top;

		const scaleFactor = e.deltaY < 0 ? 1.1 : 0.9;
		const newK = Math.max(0.1, Math.min(5, transform.k * scaleFactor));

		// Zoom toward cursor
		transform = {
			x: sx - ((sx - transform.x) / transform.k) * newK,
			y: sy - ((sy - transform.y) / transform.k) * newK,
			k: newK,
		};
		draw();
	}

	// --- Resize ---

	function handleResize() {
		if (!container || !canvas) return;
		const rect = container.getBoundingClientRect();
		width = rect.width;
		height = rect.height;
		const dpr = window.devicePixelRatio || 1;
		canvas.width = width * dpr;
		canvas.height = height * dpr;
		canvas.style.width = `${width}px`;
		canvas.style.height = `${height}px`;

		if (simulation) {
			simulation.force('center', forceCenter(width / 2, height / 2));
			simulation.alpha(0.3).restart();
		}
		draw();
	}

	// --- Helpers ---

	function formatEntityType(t: string): string {
		return t.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());
	}

	function formatRelType(t: string): string {
		return t.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());
	}

	function getEntityColor(t: string): string {
		return ENTITY_COLORS[t] ?? '#6b7280';
	}

	function getRelColor(t: string): string {
		return RELATIONSHIP_COLORS[t] ?? '#6b7280';
	}

	// --- Lifecycle ---

	onMount(() => {
		handleResize();
		window.addEventListener('resize', handleResize);
		loadGraph();
	});

	onDestroy(() => {
		if (simulation) simulation.stop();
		if (animFrame) cancelAnimationFrame(animFrame);
		window.removeEventListener('resize', handleResize);
	});

	// Derived stats
	const entityTypeCounts = $derived.by(() => {
		const counts: Record<string, number> = {};
		for (const n of nodes) {
			counts[n.entity_type] = (counts[n.entity_type] || 0) + 1;
		}
		return Object.entries(counts).sort((a, b) => b[1] - a[1]);
	});
</script>

<div class="flex h-full flex-col">
	<!-- Top bar -->
	<div
		class="flex h-10 shrink-0 items-center justify-between border-b border-border-default px-4"
	>
		<div class="flex items-center gap-3">
			<a
				href="/"
				class="flex items-center gap-1.5 rounded-md px-2 py-1 text-xs text-text-secondary hover:bg-bg-surface hover:text-text-primary"
			>
				<svg class="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="2"
						d="M15 19l-7-7 7-7"
					/>
				</svg>
				Dashboard
			</a>
			<span class="text-sm font-semibold text-text-primary">Entity Graph</span>
			{#if !loading}
				<span class="text-xs text-text-muted"
					>{nodes.length} entities, {links.length} relationships</span
				>
			{/if}
		</div>
		<div class="flex items-center gap-2">
			<button
				onclick={() => {
					transform = { x: 0, y: 0, k: 1 };
					if (simulation) {
						simulation.force('center', forceCenter(width / 2, height / 2));
						simulation.alpha(0.8).restart();
					}
					draw();
				}}
				class="rounded-md px-2 py-1 text-xs text-text-secondary hover:bg-bg-surface hover:text-text-primary"
			>
				Reset View
			</button>
			<button
				onclick={() => loadGraph()}
				class="rounded-md px-2 py-1 text-xs text-text-secondary hover:bg-bg-surface hover:text-text-primary"
			>
				Reload
			</button>
		</div>
	</div>

	<!-- Main content -->
	<div class="relative min-h-0 flex-1">
		{#if loading}
			<div class="flex h-full items-center justify-center">
				<div class="text-center">
					<div
						class="mx-auto mb-3 h-8 w-8 animate-spin rounded-full border-2 border-accent border-t-transparent"
					></div>
					<p class="text-sm text-text-muted">Loading entity graph...</p>
				</div>
			</div>
		{:else if error}
			<div class="flex h-full items-center justify-center">
				<div class="text-center">
					<p class="mb-2 text-sm text-alert">{error}</p>
					<button
						onclick={() => loadGraph()}
						class="rounded-md bg-bg-surface px-3 py-1.5 text-xs text-text-primary hover:bg-bg-card-hover"
					>
						Retry
					</button>
				</div>
			</div>
		{:else if nodes.length === 0}
			<div class="flex h-full items-center justify-center">
				<div class="text-center">
					<p class="mb-1 text-sm text-text-muted">No entities found</p>
					<p class="text-xs text-text-muted">
						Entities will appear as the pipeline processes events.
					</p>
				</div>
			</div>
		{:else}
			<!-- Canvas -->
			<div bind:this={container} class="absolute inset-0">
				<canvas
					bind:this={canvas}
					onmousemove={handleMouseMove}
					onmousedown={handleMouseDown}
					onmouseup={handleMouseUp}
					onmouseleave={handleMouseUp}
					onclick={handleClick}
					onwheel={handleWheel}
					class="cursor-grab"
				></canvas>
			</div>

			<!-- Legend -->
			<div
				class="absolute bottom-4 left-4 rounded-lg border border-border-default bg-bg-card/90 p-3 backdrop-blur-sm"
			>
				<p class="mb-2 text-[10px] font-semibold uppercase tracking-wider text-text-muted">
					Entity Types
				</p>
				<div class="space-y-1">
					{#each entityTypeCounts as [type, count] (type)}
						<div class="flex items-center gap-2">
							<span
								class="inline-block h-2.5 w-2.5 rounded-full"
								style="background: {getEntityColor(type)}"
							></span>
							<span class="text-[11px] text-text-secondary"
								>{formatEntityType(type)}</span
							>
							<span class="text-[10px] text-text-muted">{count}</span>
						</div>
					{/each}
				</div>
				{#if links.length > 0}
					<p
						class="mb-1.5 mt-3 text-[10px] font-semibold uppercase tracking-wider text-text-muted"
					>
						Relationship Types
					</p>
					<div class="space-y-1">
						{#each Object.entries(RELATIONSHIP_COLORS) as [type, color] (type)}
							{@const count = links.filter((l) => l.relationship === type).length}
							{#if count > 0}
								<div class="flex items-center gap-2">
									<span
										class="inline-block h-0.5 w-3"
										style="background: {color}; {DASHED_RELATIONSHIPS.has(type)
											? 'border-bottom: 1px dashed ' + color + '; background: transparent;'
											: ''}"
									></span>
									<span class="text-[11px] text-text-secondary"
										>{formatRelType(type)}</span
									>
									<span class="text-[10px] text-text-muted">{count}</span>
								</div>
							{/if}
						{/each}
					</div>
				{/if}
			</div>

			<!-- Hover tooltip -->
			{#if hoveredNode && !selectedNode}
				<div
					class="pointer-events-none absolute right-4 top-4 w-64 rounded-lg border border-border-default bg-bg-card/95 p-3 backdrop-blur-sm"
				>
					<div class="flex items-center gap-2">
						<span
							class="inline-block h-2.5 w-2.5 rounded-full"
							style="background: {getEntityColor(hoveredNode.entity_type)}"
						></span>
						<span class="text-sm font-medium text-text-primary"
							>{hoveredNode.canonical_name}</span
						>
					</div>
					<div class="mt-1 text-xs text-text-muted">
						{formatEntityType(hoveredNode.entity_type)} &middot;
						{hoveredNode.mention_count} mentions
					</div>
				</div>
			{/if}

			<!-- Detail panel -->
			{#if selectedNode}
				<div
					class="absolute right-4 top-4 w-80 max-h-[calc(100%-2rem)] overflow-y-auto rounded-lg border border-border-default bg-bg-card/95 backdrop-blur-sm"
				>
					<div
						class="flex items-start justify-between border-b border-border-default p-3"
					>
						<div>
							<div class="flex items-center gap-2">
								<span
									class="inline-block h-3 w-3 rounded-full"
									style="background: {getEntityColor(selectedNode.entity_type)}"
								></span>
								<span class="text-sm font-semibold text-text-primary"
									>{selectedNode.canonical_name}</span
								>
							</div>
							<div class="mt-0.5 text-xs text-text-muted">
								{formatEntityType(selectedNode.entity_type)}
							</div>
						</div>
						<button
							onclick={() => {
								selectedNode = null;
								selectedDetail = null;
								draw();
							}}
							aria-label="Close detail panel"
							class="rounded p-1 text-text-muted hover:bg-bg-surface hover:text-text-primary"
						>
							<svg
								class="h-4 w-4"
								fill="none"
								stroke="currentColor"
								viewBox="0 0 24 24"
							>
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M6 18L18 6M6 6l12 12"
								/>
							</svg>
						</button>
					</div>

					<div class="space-y-3 p-3">
						<!-- Stats -->
						<div class="grid grid-cols-2 gap-2">
							<div class="rounded bg-bg-surface px-2 py-1.5">
								<div class="text-[10px] text-text-muted">Mentions</div>
								<div class="text-sm font-medium text-text-primary">
									{selectedNode.mention_count}
								</div>
							</div>
							<div class="rounded bg-bg-surface px-2 py-1.5">
								<div class="text-[10px] text-text-muted">Confidence</div>
								<div class="text-sm font-medium text-text-primary">
									{(selectedNode.confidence * 100).toFixed(0)}%
								</div>
							</div>
						</div>

						<!-- Wikidata -->
						{#if selectedNode.wikidata_id}
							<div>
								<p class="text-[10px] font-semibold uppercase text-text-muted">
									Wikidata
								</p>
								<a
									href="https://www.wikidata.org/wiki/{selectedNode.wikidata_id}"
									target="_blank"
									rel="noopener noreferrer"
									class="text-xs text-accent hover:underline"
								>
									{selectedNode.wikidata_id}
								</a>
							</div>
						{/if}

						<!-- Aliases -->
						{#if selectedNode.aliases.length > 0}
							<div>
								<p class="text-[10px] font-semibold uppercase text-text-muted">
									Aliases
								</p>
								<div class="mt-1 flex flex-wrap gap-1">
									{#each selectedNode.aliases as alias (alias)}
										<span
											class="rounded bg-bg-surface px-1.5 py-0.5 text-[11px] text-text-secondary"
											>{alias}</span
										>
									{/each}
								</div>
							</div>
						{/if}

						<!-- Relationships -->
						{#if detailLoading}
							<div class="py-2 text-center text-xs text-text-muted">
								Loading detail...
							</div>
						{:else if selectedDetail?.relationships && selectedDetail.relationships.length > 0}
							<div>
								<p class="text-[10px] font-semibold uppercase text-text-muted">
									Relationships ({selectedDetail.relationships.length})
								</p>
								<div class="mt-1 space-y-1">
									{#each selectedDetail.relationships.slice(0, 20) as rel (rel.id)}
										{@const otherEntityId =
											rel.source_entity === selectedNode.id
												? rel.target_entity
												: rel.source_entity}
										{@const otherNode = nodes.find(
											(n) => n.id === otherEntityId,
										)}
										<div
											class="flex items-center gap-1.5 rounded bg-bg-surface px-2 py-1"
										>
											<span
												class="inline-block h-1.5 w-1.5 rounded-full"
												style="background: {getRelColor(rel.relationship)}"
											></span>
											<span class="text-[11px] text-text-secondary">
												{formatRelType(rel.relationship)}
											</span>
											<span class="text-[11px] text-text-muted">&rarr;</span>
											<span class="truncate text-[11px] text-text-primary">
												{otherNode?.canonical_name ?? 'Unknown'}
											</span>
										</div>
									{/each}
								</div>
							</div>
						{/if}

						<!-- State changes -->
						{#if selectedDetail?.state_changes && selectedDetail.state_changes.length > 0}
							<div>
								<p class="text-[10px] font-semibold uppercase text-text-muted">
									State Changes
								</p>
								<div class="mt-1 space-y-1">
									{#each selectedDetail.state_changes.slice(0, 10) as change (change.id)}
										<div
											class="rounded bg-bg-surface px-2 py-1"
										>
											<div class="flex items-center gap-1.5">
												<span
													class="rounded px-1 py-0.5 text-[10px] font-medium
													{change.certainty === 'confirmed'
														? 'bg-success/20 text-success'
														: change.certainty === 'denied'
															? 'bg-alert/20 text-alert'
															: 'bg-warning/20 text-warning'}"
												>
													{change.certainty}
												</span>
												<span class="text-[11px] text-text-primary">
													{change.change_type.replace(/_/g, ' ')}
												</span>
											</div>
											<div class="mt-0.5 text-[10px] text-text-muted">
												{new Date(change.detected_at).toLocaleDateString()}
											</div>
										</div>
									{/each}
								</div>
							</div>
						{/if}
					</div>
				</div>
			{/if}
		{/if}
	</div>
</div>
