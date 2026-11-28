using System;
using System.Collections.Generic;
using System.Globalization;

namespace VibeUsage.App.Panel;

public readonly record struct PathPoint(double X, double Y);

public abstract record PathSegmentData(PathPoint To)
{
    public sealed record Line(PathPoint To) : PathSegmentData(To);
    public sealed record Cubic(PathPoint Control1, PathPoint Control2, PathPoint To) : PathSegmentData(To);
    public sealed record Arc(PathPoint To, double RadiusX, double RadiusY, double Rotation, bool LargeArc, bool Clockwise)
        : PathSegmentData(To);
}

public sealed record PathFigureData(PathPoint Start, IReadOnlyList<PathSegmentData> Segments, bool Closed);

public sealed record PathData(bool EvenOdd, IReadOnlyList<PathFigureData> Figures);

/// <summary>
/// Reads SVG / XAML path markup (the provider marks' data) into figures, so
/// the panel builds geometry in code instead of handing strings to the XAML
/// parser at run time. Supports F0/F1 and M L H V C S Q A Z, absolute and
/// relative, with implicit repeats.
/// </summary>
public static class PathMarkup
{
    public static PathData Parse(string markup)
    {
        var reader = new Reader(markup);
        bool evenOdd = false;
        var figures = new List<PathFigureData>();
        List<PathSegmentData>? segments = null;
        PathPoint start = default, current = default, lastControl = default;
        bool hasLastCubic = false;
        char command = '\0';

        void Begin(PathPoint at)
        {
            Finish(false);
            segments = [];
            start = current = at;
        }

        void Finish(bool closed)
        {
            if (segments is { } s) figures.Add(new(start, s, closed));
            segments = null;
        }

        void Add(PathSegmentData segment)
        {
            if (segments == null) Begin(current);
            segments!.Add(segment);
            current = segment.To;
        }

        while (reader.More())
        {
            if (reader.TryCommand(out var next))
            {
                command = next;
                if (command is 'F' or 'f')
                {
                    evenOdd = reader.Number() == 0;
                    command = '\0';
                    continue;
                }
                if (command is 'Z' or 'z')
                {
                    Finish(true);
                    current = start;
                    hasLastCubic = false;
                    command = '\0';
                    continue;
                }
            }
            else if (command == '\0')
            {
                throw new FormatException("Path markup: number without a command");
            }

            bool relative = char.IsLower(command);
            PathPoint Point(double x, double y) => relative ? new(current.X + x, current.Y + y) : new(x, y);
            switch (char.ToUpperInvariant(command))
            {
                case 'M':
                    Begin(Point(reader.Number(), reader.Number()));
                    command = relative ? 'l' : 'L'; // further pairs are lines
                    hasLastCubic = false;
                    break;
                case 'L':
                    Add(new PathSegmentData.Line(Point(reader.Number(), reader.Number())));
                    hasLastCubic = false;
                    break;
                case 'H':
                {
                    double x = reader.Number();
                    Add(new PathSegmentData.Line(new(relative ? current.X + x : x, current.Y)));
                    hasLastCubic = false;
                    break;
                }
                case 'V':
                {
                    double y = reader.Number();
                    Add(new PathSegmentData.Line(new(current.X, relative ? current.Y + y : y)));
                    hasLastCubic = false;
                    break;
                }
                case 'C':
                {
                    var c1 = Point(reader.Number(), reader.Number());
                    var c2 = Point(reader.Number(), reader.Number());
                    var to = Point(reader.Number(), reader.Number());
                    Add(new PathSegmentData.Cubic(c1, c2, to));
                    lastControl = c2;
                    hasLastCubic = true;
                    break;
                }
                case 'S':
                {
                    var c1 = hasLastCubic ? new PathPoint(2 * current.X - lastControl.X, 2 * current.Y - lastControl.Y) : current;
                    var c2 = Point(reader.Number(), reader.Number());
                    var to = Point(reader.Number(), reader.Number());
                    Add(new PathSegmentData.Cubic(c1, c2, to));
                    lastControl = c2;
                    hasLastCubic = true;
                    break;
                }
                case 'Q':
                {
                    var q = Point(reader.Number(), reader.Number());
                    var to = Point(reader.Number(), reader.Number());
                    var from = current;
                    var c1 = new PathPoint(from.X + 2.0 / 3 * (q.X - from.X), from.Y + 2.0 / 3 * (q.Y - from.Y));
                    var c2 = new PathPoint(to.X + 2.0 / 3 * (q.X - to.X), to.Y + 2.0 / 3 * (q.Y - to.Y));
                    Add(new PathSegmentData.Cubic(c1, c2, to));
                    hasLastCubic = false;
                    break;
                }
                case 'A':
                {
                    double rx = reader.Number(), ry = reader.Number(), rotation = reader.Number();
                    bool large = reader.Flag(), sweep = reader.Flag();
                    var to = Point(reader.Number(), reader.Number());
                    Add(new PathSegmentData.Arc(to, rx, ry, rotation, large, sweep));
                    hasLastCubic = false;
                    break;
                }
                default:
                    throw new FormatException($"Path markup: unsupported command '{command}'");
            }
        }
        Finish(false);
        return new(evenOdd, figures);
    }

    private sealed class Reader(string text)
    {
        private int i;

        private void SkipSeparators()
        {
            while (i < text.Length && (char.IsWhiteSpace(text[i]) || text[i] == ',')) i++;
        }

        public bool More()
        {
            SkipSeparators();
            return i < text.Length;
        }

        public bool TryCommand(out char command)
        {
            SkipSeparators();
            command = '\0';
            if (i >= text.Length || !char.IsAsciiLetter(text[i]) || text[i] is 'e' or 'E') return false;
            command = text[i++];
            return true;
        }

        /// <summary>An arc flag: a single 0 or 1, which SVG lets run into the next number ("0 00-.856").</summary>
        public bool Flag()
        {
            SkipSeparators();
            if (i < text.Length && text[i] is '0' or '1') return text[i++] == '1';
            throw new FormatException($"Path markup: arc flag expected at {i}");
        }

        public double Number()
        {
            SkipSeparators();
            int begin = i;
            if (i < text.Length && text[i] is '+' or '-') i++;
            bool dot = false;
            while (i < text.Length && (char.IsAsciiDigit(text[i]) || text[i] == '.' && !dot))
            {
                if (text[i] == '.') dot = true;
                i++;
            }
            if (i < text.Length && text[i] is 'e' or 'E')
            {
                i++;
                if (i < text.Length && text[i] is '+' or '-') i++;
                while (i < text.Length && char.IsAsciiDigit(text[i])) i++;
            }
            if (i == begin) throw new FormatException($"Path markup: number expected at {begin}");
            return double.Parse(text.AsSpan(begin, i - begin), NumberStyles.Float, CultureInfo.InvariantCulture);
        }
    }
}
