use std::collections::HashSet;

use crate::{
    DomainResult,
    model::{Mirror, MirrorKind, Region},
};

const CATALOG: &str = r#"
cn-tsinghua,cn,https://mirrors.tuna.tsinghua.edu.cn/archlinux/
cn-ustc,cn,https://mirrors.ustc.edu.cn/archlinux/
cn-aliyun,cn,https://mirrors.aliyun.com/archlinux/
cn-jlu,cn,https://mirrors.jlu.edu.cn/archlinux/
cn-nju,cn,https://mirrors.nju.edu.cn/archlinux/
cn-xjtu,cn,https://mirrors.xjtu.edu.cn/archlinux/
cn-hust,cn,https://mirrors.hust.edu.cn/archlinux/
cn-nyist,cn,https://mirror.nyist.edu.cn/archlinux/
hk-cicku,hk,https://hk.mirrors.cicku.me/archlinux/
hk-rackspace,hk,https://hkg.mirror.rackspace.com/archlinux/
hk-koddos,hk,https://mirror-hk.koddos.net/archlinux/
jp-miraa,jp,https://www.miraa.jp/archlinux/
jp-aria,jp,https://mirror.aria-on-the-planet.es/archlinux/
jp-yamagata,jp,https://ftp.yz.yamagata-u.ac.jp/pub/linux/archlinux/
jp-jaist,jp,https://ftp.jaist.ac.jp/pub/Linux/ArchLinux/
jp-cicku,jp,https://jp.mirrors.cicku.me/archlinux/
jp-rain,jp,https://mirror.rain.ne.jp/archlinux/
jp-hashy,jp,https://mirror.hashy0917.net/archlinux/
us-mtu,us,https://mirrors.lug.mtu.edu/archlinux/
us-rit,us,https://mirrors.rit.edu/archlinux/
us-umd,us,https://mirror.umd.edu/archlinux/
us-osuosl,us,https://ftp.osuosl.org/pub/archlinux/
us-pkgbuild-la,us,https://losangeles.mirror.pkgbuild.com/
us-cicku,us,https://us.mirrors.cicku.me/archlinux/
us-constant,us,https://arch.mirror.constant.com/
us-tzulo,us,https://mirror.tzulo.com/archlinux/
us-misaka-iad,us,https://iad.mirrors.misaka.one/archlinux/
us-berkeley,us,https://mirrors.ocf.berkeley.edu/archlinux/
global-cloudflare,global,https://cloudflaremirrors.com/archlinux/
"#;

pub fn built_in() -> Vec<Mirror> {
    CATALOG
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let (name, rest) = line.split_once(',')?;
            let (region, base) = rest.split_once(',')?;
            Some((name, region, base))
        })
        .map(|(name, region, base)| {
            let base = base.trim_end_matches('/');
            Mirror {
                name: name.to_owned(),
                kind: MirrorKind::Pacman,
                region: Region::parse(region).unwrap_or_default(),
                url: format!("{base}/$repo/os/$arch"),
                test_url: format!("{base}/core/os/x86_64/core.db.tar.gz"),
                priority: 0,
                enabled: true,
            }
        })
        .collect()
}

pub fn select(mirrors: &[Mirror], region: Option<Region>) -> Vec<Mirror> {
    let mut selected: Vec<Mirror> = mirrors
        .iter()
        .filter(|mirror| mirror.enabled && region.is_none_or(|wanted| mirror.region == wanted))
        .cloned()
        .collect();
    selected.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.name.cmp(&right.name))
    });
    selected
}

pub fn validate(mirrors: &[Mirror]) -> DomainResult<()> {
    let mut names = HashSet::new();
    for (index, mirror) in mirrors.iter().enumerate() {
        if mirror.name.trim().is_empty()
            || mirror.url.trim().is_empty()
            || mirror.test_url.trim().is_empty()
        {
            return Err(crate::DomainError::config(format!(
                "mirrors[{index}] needs name, url, and test_url"
            )));
        }
        if !names.insert(mirror.name.clone()) {
            return Err(crate::DomainError::config(format!(
                "mirrors[{index}] repeats name {:?}",
                mirror.name
            )));
        }
        crate::config::validate_url(&format!("mirrors[{index}].url"), &mirror.url)?;
        crate::config::validate_url(&format!("mirrors[{index}].test_url"), &mirror.test_url)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_four_regions() {
        let catalog = built_in();
        assert_eq!(catalog.len(), 29);
        for region in Region::FOCUS {
            assert!(
                catalog
                    .iter()
                    .filter(|mirror| mirror.region == region)
                    .count()
                    >= 3
            );
        }
        assert!(
            catalog
                .iter()
                .all(|mirror| mirror.test_url.ends_with("/core/os/x86_64/core.db.tar.gz"))
        );
    }

    #[test]
    fn region_selection_is_sorted_and_filtered() {
        let catalog = built_in();
        let selected = select(&catalog, Some(Region::Hk));
        assert_eq!(selected.len(), 3);
        assert!(selected.iter().all(|mirror| mirror.region == Region::Hk));
    }
}
