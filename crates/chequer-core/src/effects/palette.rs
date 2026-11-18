use crate::raster::Raster;

pub type Rgb = [u8; 3];

const MAX_SAMPLES: usize = 65_536;
const KMEANS_SAMPLES: usize = 16_384;
const KMEANS_ITERATIONS: usize = 6;
const MID_CHANNEL: u8 = 128;
const MAX_CHANNEL: u32 = 255;
const UNIFORM_CHANNEL_ORDER: [usize; 3] = [1, 0, 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    MedianCut,
    KMeans,
    Uniform,
}

impl Method {
    pub fn from_key(key: &str) -> Self {
        match key {
            "kmeans" => Method::KMeans,
            "uniform_grid" => Method::Uniform,
            _ => Method::MedianCut,
        }
    }

    pub fn build(self, samples: Vec<Rgb>, count: usize) -> Vec<Rgb> {
        match self {
            Method::MedianCut => median_cut(samples, count),
            Method::KMeans => k_means(samples, count),
            Method::Uniform => uniform(&samples, count),
        }
    }
}

fn median_cut(samples: Vec<Rgb>, count: usize) -> Vec<Rgb> {
    let spread = |pixels: &[Rgb]| -> (usize, u8) {
        (0..3)
            .map(|c| {
                let (low, high) = pixels.iter().fold((u8::MAX, 0), |(low, high), p| (low.min(p[c]), high.max(p[c])));
                (c, high - low)
            })
            .max_by_key(|&(_, width)| width)
            .unwrap_or((0, 0))
    };
    let mut boxes = vec![samples];
    while boxes.len() < count {
        let Some((index, _)) = boxes.iter().enumerate().filter(|(_, b)| b.len() > 1).max_by_key(|(_, b)| u32::from(spread(b).1) * b.len() as u32) else { break };
        let mut widest = boxes.swap_remove(index);
        let (channel, width) = spread(&widest);
        if width == 0 {
            boxes.push(widest);
            break;
        }
        widest.sort_unstable_by_key(|p| p[channel]);
        let upper = widest.split_off(widest.len() / 2);
        boxes.extend([widest, upper]);
    }
    boxes
        .iter()
        .filter(|b| !b.is_empty())
        .map(|b| std::array::from_fn(|c| (b.iter().map(|p| u32::from(p[c])).sum::<u32>() / b.len() as u32) as u8))
        .collect()
}

fn k_means(samples: Vec<Rgb>, count: usize) -> Vec<Rgb> {
    let stride = (samples.len() / KMEANS_SAMPLES).max(1);
    let reduced: Vec<Rgb> = samples.iter().step_by(stride).copied().collect();
    let mut centers = median_cut(reduced.clone(), count);
    for _ in 0..KMEANS_ITERATIONS {
        let mut sums = vec![[0u64; 3]; centers.len()];
        let mut members = vec![0u64; centers.len()];
        for pixel in &reduced {
            let owner = nearest_index(&centers, pixel.map(f32::from));
            members[owner] += 1;
            for c in 0..3 {
                sums[owner][c] += u64::from(pixel[c]);
            }
        }
        for ((center, sum), &size) in centers.iter_mut().zip(&sums).zip(&members) {
            if size > 0 {
                *center = std::array::from_fn(|c| (sum[c] / size) as u8);
            }
        }
    }
    centers
}

fn channel_levels(count: usize) -> [u32; 3] {
    let mut levels = [1u32; 3];
    loop {
        let grown = UNIFORM_CHANNEL_ORDER.into_iter().min_by_key(|&c| levels[c]).unwrap_or(0);
        let total: u32 = levels.iter().product::<u32>() / levels[grown] * (levels[grown] + 1);
        if total as usize > count {
            return levels;
        }
        levels[grown] += 1;
    }
}

fn uniform(samples: &[Rgb], count: usize) -> Vec<Rgb> {
    if samples.is_empty() {
        return Vec::new();
    }
    let levels = channel_levels(count);
    let value = |level: u32, steps: u32| if steps == 1 { MID_CHANNEL } else { (level * MAX_CHANNEL / (steps - 1)) as u8 };
    let mut palette = Vec::new();
    for r in 0..levels[0] {
        for g in 0..levels[1] {
            for b in 0..levels[2] {
                palette.push([value(r, levels[0]), value(g, levels[1]), value(b, levels[2])]);
            }
        }
    }
    palette
}

fn nearest_index(palette: &[Rgb], color: [f32; 3]) -> usize {
    palette
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| distance(a, color).total_cmp(&distance(b, color)))
        .map_or(0, |(index, _)| index)
}

pub fn nearest(palette: &[Rgb], color: [f32; 3]) -> Rgb {
    palette.get(nearest_index(palette, color)).copied().unwrap_or_default()
}

fn distance(entry: &Rgb, color: [f32; 3]) -> f32 {
    (0..3).map(|c| (f32::from(entry[c]) - color[c]).powi(2)).sum()
}

pub fn sample_colors(source: &Raster) -> Vec<Rgb> {
    let total = (source.width() * source.height()) as usize;
    let stride = (total / MAX_SAMPLES).max(1);
    (0..source.height())
        .flat_map(|y| (0..source.width()).map(move |x| (x, y)))
        .step_by(stride)
        .map(|(x, y)| source.get(x, y))
        .filter(|p| p[3] > 0)
        .map(|p| [p[0], p[1], p[2]])
        .collect()
}
