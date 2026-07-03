// Fixture: typed functions, methods, arrows, small bodies.

interface Point {
  x: number;
  y: number;
}

class Path {
  private points: Point[] = [];

  length(): number {
    // sum segment lengths
    let total = 0;
    for (let i = 1; i < this.points.length; i++) {
      const dx = this.points[i].x - this.points[i - 1].x;
      const dy = this.points[i].y - this.points[i - 1].y;
      total += Math.sqrt(dx * dx + dy * dy);
    }
    return total;
  }
}

export const centroid = (points: Point[]): Point => {
  const sum = points.reduce(
    (acc, p) => ({ x: acc.x + p.x, y: acc.y + p.y }),
    { x: 0, y: 0 },
  );
  return { x: sum.x / points.length, y: sum.y / points.length };
};

const tiny = (): number => 1;
