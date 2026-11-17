import clsx from 'clsx';
import Link from '@docusaurus/Link';
import styles from './styles.module.css';

type FeatureItem = {
  title: string;
  img: string;
  link: string;
  description: JSX.Element;
};

const FeatureList: FeatureItem[] = [
  {
    title: 'One set of keys for every backend',
    img: require('@site/static/gif/navigation.gif').default,
    link: '/docs/usage/navigation',
    description: (
      <>
        Postgres, MySQL, SQLite, MongoDB, Redis and RabbitMQ: connections, databases,
        tables, rows. Learn the keys once.
      </>
    ),
  },
  {
    title: 'Query in their own language',
    img: require('@site/static/gif/query.gif').default,
    link: '/docs/usage/querying',
    description: (
      <>
        SQL, Mongo filters and pipelines, Redis commands, publish to a queue. History
        kept per connection.
      </>
    ),
  },
  {
    title: 'Edit without surprises',
    img: require('@site/static/gif/editing.gif').default,
    link: '/docs/usage/editing',
    description: (
      <>
        Edit field by field, insert, delete: confirmations show the statement that
        will run, read-only connections refuse writes for real.
      </>
    ),
  },
];

function Feature({ title, img, link, description }: FeatureItem) {
  return (
    <div className={clsx('col col--4')}>
      <Link to={link} className={styles.featureLink}>
        <img src={img} alt={title} className={styles.cardImg} loading="lazy" />
        <h3 className={styles.cardTitle}>{title}</h3>
        <p className={styles.cardDesc}>{description}</p>
      </Link>
    </div>
  );
}

export default function HomepageFeatures(): JSX.Element {
  return (
    <section className={styles.features}>
      <div className="container">
        <h2 className={styles.sectionTitle}>Why db9s?</h2>
        <div className="row">
          {FeatureList.map((props, idx) => (
            <Feature key={idx} {...props} />
          ))}
        </div>
        <p className={styles.sectionTitle} style={{ fontSize: '1.1rem', marginTop: '2rem' }}>
          Prefer a window? The <Link to="/docs/desktop-app">desktop app</Link> has the
          same screens, plus a grid, a JSON inspector, tabs and live charts.
        </p>
      </div>
    </section>
  );
}
