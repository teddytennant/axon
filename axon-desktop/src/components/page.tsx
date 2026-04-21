import type { ReactNode } from 'react';

/**
 * Shared page primitives.
 *
 * Every page was re-implementing the same header, empty-state, and skeleton
 * wrappers by hand — these live here now so layout tweaks propagate. The
 * `Skeleton` variants accept a prop bag instead of being re-declared per page.
 */

export function PageHeader({
  label,
  count,
  children,
}: {
  label: string;
  count?: number;
  children?: ReactNode;
}) {
  return (
    <div className="mb-6 flex items-baseline gap-3">
      <span className="text-[11px] font-medium tracking-wider text-[#555]">{label}</span>
      {count != null && (
        <span className="text-[10px] tabular-nums text-[#2e2e2e]">{count}</span>
      )}
      {children}
    </div>
  );
}

export function EmptyState({ text, height = 48 }: { text: string; height?: 40 | 48 }) {
  const hClass = height === 40 ? 'h-40' : 'h-48';
  return (
    <div className={`flex ${hClass} items-center justify-center`}>
      <p className="text-[11px] text-[#1e1e1e]">{text}</p>
    </div>
  );
}

/**
 * Configurable grid skeleton shared by mesh/agents/trust/tasks while data is
 * loading. `statsCount` renders a row of square stat placeholders above the
 * main grid; pass 0 to skip it. `cards` controls the card grid, pass `null`
 * for a single full-width block instead.
 */
export function GridSkeleton({
  headerWidth = 16,
  statsCount = 0,
  cards = 6,
  cardHeight = 28,
}: {
  headerWidth?: 16 | 20;
  statsCount?: number;
  cards?: number | null;
  cardHeight?: 28 | 36 | 48;
}) {
  const hw = headerWidth === 20 ? 'w-20' : 'w-16';
  const ch = cardHeight === 36 ? 'h-36' : cardHeight === 48 ? 'h-48' : 'h-28';
  return (
    <div className="p-6">
      <div className={`mb-6 h-4 ${hw} rounded animate-shimmer`} />
      {statsCount > 0 && (
        <div className="mb-6 grid grid-cols-5 gap-2">
          {Array.from({ length: statsCount }).map((_, i) => (
            <div key={i} className="h-20 rounded-lg border border-[#141414] animate-shimmer" />
          ))}
        </div>
      )}
      {cards === null ? (
        <div className="h-48 rounded-lg border border-[#141414] animate-shimmer" />
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {Array.from({ length: cards }).map((_, i) => (
            <div key={i} className={`${ch} rounded-lg border border-[#141414] animate-shimmer`} />
          ))}
        </div>
      )}
    </div>
  );
}
