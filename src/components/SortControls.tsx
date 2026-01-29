import type { SortConfig, SortField } from '../types';

interface SortControlsProps {
  sortConfig: SortConfig;
  onSort: (field: SortField) => void;
}

export function SortControls({ sortConfig, onSort }: SortControlsProps) {
  const getSortIndicator = (field: SortField) => {
    if (sortConfig.field !== field) return null;
    return sortConfig.direction === 'asc' ? ' ↑' : ' ↓';
  };

  const buttonClass = (field: SortField) => `
    px-3 py-1.5 rounded text-sm font-medium transition-colors
    ${sortConfig.field === field
      ? 'bg-blue-600 text-white'
      : 'bg-gray-200 text-gray-700 hover:bg-gray-300'}
  `;

  return (
    <div className="flex items-center gap-2">
      <span className="text-sm text-gray-500 mr-2">Sort by:</span>
      <button
        onClick={() => onSort('name')}
        className={buttonClass('name')}
      >
        Name{getSortIndicator('name')}
      </button>
      <button
        onClick={() => onSort('size')}
        className={buttonClass('size')}
      >
        Size{getSortIndicator('size')}
      </button>
      <button
        onClick={() => onSort('path')}
        className={buttonClass('path')}
      >
        Path{getSortIndicator('path')}
      </button>
    </div>
  );
}
